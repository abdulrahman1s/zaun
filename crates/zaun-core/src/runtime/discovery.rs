//! Discovery of Proton builds and Steam Linux Runtime installs (RUNTIME_MANAGEMENT §5).
//!
//! Scans the store and the well-known external locations without following symlinks out of
//! the scanned directory, validates each candidate (rules 1–5), and detects the SLR variants
//! of the store. umu's own runtime under `~/.local/share/umu` and its partial downloads in
//! `~/.cache/umu` are reported for the Doctor screen only: SLR installs are only ever taken
//! from the store.

use std::path::{Path, PathBuf};

use super::store::{RuntimeStore, family_of, version_key};
use super::vdf::{self, VdfObject};
use crate::clock::Timestamp;
use crate::fsx::{FileType, Fs};
use crate::ids::RuntimeName;
use crate::model::{RuntimeBuild, RuntimeFamily, RuntimeOrigin, SlrVariant};

/// A Proton build in the catalogue (MODULES §5 `ProtonBuild`; the `runtimes` row type).
pub type ProtonBuild = RuntimeBuild;

/// Cap on entries read per location.
const MAX_ENTRIES: usize = 512;
const SMALL_FILE: u64 = 64 * 1024;

/// Map `require_tool_appid` to an SLR variant (F-105). Soldier (`1391110`) and the arm64
/// runtime are not supported in 1.0.
pub fn slr_for_appid(appid: &str) -> Option<SlrVariant> {
    match appid.trim() {
        "1628350" => Some(SlrVariant::Steamrt3),
        "4183110" => Some(SlrVariant::Steamrt4),
        _ => None,
    }
}

/// One location to scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// Directory whose children are candidate builds.
    pub dir: PathBuf,
    /// Store or external.
    pub origin: RuntimeOrigin,
    /// Shown as ("Managed by Zaun", "From Steam", …).
    pub label: &'static str,
}

/// The scan order of §5: store first, then Steam, system packages, Heroic, Lutris, custom.
pub fn locations(store: &RuntimeStore, home: Option<&Path>, extra: &[PathBuf]) -> Vec<Location> {
    let mut v = vec![Location {
        dir: store.compat.clone(),
        origin: RuntimeOrigin::Store,
        label: "Managed by Zaun",
    }];
    let ext = |dir: PathBuf, label| Location {
        dir,
        origin: RuntimeOrigin::External,
        label,
    };
    if let Some(h) = home {
        for rel in [
            ".local/share/Steam/compatibilitytools.d",
            ".steam/root/compatibilitytools.d",
            ".steam/steam/compatibilitytools.d",
        ] {
            v.push(ext(h.join(rel), "From Steam"));
        }
    }
    for sys in [
        "/usr/share/steam/compatibilitytools.d",
        "/usr/local/share/steam/compatibilitytools.d",
    ] {
        v.push(ext(PathBuf::from(sys), "System package"));
    }
    if let Some(h) = home {
        v.push(ext(h.join(".config/heroic/tools/proton"), "From Heroic"));
        v.push(ext(
            h.join(".local/share/lutris/runners/proton"),
            "From Lutris",
        ));
    }
    for d in extra.iter().take(16) {
        v.push(ext(d.clone(), "Custom"));
    }
    v
}

/// Why a candidate was not accepted.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RejectReason {
    /// Rule 1: `toolmanifest.vdf` missing or unreadable.
    #[error("toolmanifest.vdf is missing or unreadable")]
    NoManifest,
    /// Rule 1: the manifest does not parse.
    #[error("toolmanifest.vdf does not parse: {0}")]
    BadManifest(vdf::VdfError),
    /// Rule 1: not a Proton layer.
    #[error("not a Proton build (layer {layer:?}, command line {commandline:?})")]
    NotProton {
        /// `compatmanager_layer_name`.
        layer: Option<String>,
        /// `commandline`.
        commandline: Option<String>,
    },
    /// Rule 2: `proton` is missing or not a regular file.
    #[error("the proton script is missing")]
    NoProtonScript,
    /// Rule 3: no container runtime required (host runtime; `ZN-RT-013`).
    #[error("requires no container runtime")]
    HostRuntime,
    /// Rule 3: an unknown or unsupported runtime (`ZN-RT-013`).
    #[error("requires the unsupported runtime {0}")]
    UnsupportedRuntime(String),
    /// The directory name is not a valid runtime name.
    #[error("the folder name is not a valid compatibility-layer name")]
    InvalidName,
    /// Rule 5: under a deny-listed location (Flatpak application data).
    #[error("inside a location Zaun does not use")]
    Denied,
    /// A symlink leaving the scanned directory.
    #[error("a link that points outside its folder")]
    SymlinkOutside,
    /// Same name as a build found earlier.
    #[error("another build with the same name was found first")]
    Duplicate,
}

/// A rejected candidate (`ZN-RT-012` when the user selects it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    /// The directory.
    pub path: PathBuf,
    /// Why.
    pub reason: RejectReason,
}

/// An SLR variant on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlrInstall {
    /// Variant.
    pub variant: SlrVariant,
    /// `…/umu/<variant>`.
    pub path: PathBuf,
    /// Version from `VERSIONS.txt` (`depot` line), else from the platform directory name.
    pub version: Option<String>,
    /// The platform directory (`sniper_platform_<version>`), host path.
    pub platform_dir: Option<PathBuf>,
    /// `.installed.ok` is a regular file.
    pub marker: bool,
    /// The `umu` link to `_v2-entry-point` exists.
    pub umu_link: bool,
}

impl SlrInstall {
    /// Marker, link and platform directory are present.
    pub fn is_complete(&self) -> bool {
        self.marker && self.umu_link && self.platform_dir.is_some()
    }
}

/// umu's own installation outside the store (informational).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForeignUmu {
    /// SLR variants under `~/.local/share/umu`.
    pub slr: Vec<SlrInstall>,
    /// `*.parts` partial downloads under `~/.cache/umu`.
    pub partial_downloads: Vec<PathBuf>,
}

/// Everything discovery found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    /// Valid builds, in location order then name order.
    pub builds: Vec<ProtonBuild>,
    /// SLR variants in the store.
    pub slr: Vec<SlrInstall>,
    /// Rejected candidates.
    pub rejected: Vec<Rejected>,
    /// umu's own runtime and cache outside the store.
    pub foreign: ForeignUmu,
}

/// Inputs of a scan.
#[derive(Clone, Debug)]
pub struct DiscoveryInput<'a> {
    /// The store.
    pub store: &'a RuntimeStore,
    /// The real home directory.
    pub home: Option<&'a Path>,
    /// `runtime.extra_search_dirs`.
    pub extra_dirs: &'a [PathBuf],
    /// Timestamp for `discovered_at`.
    pub now: Timestamp,
}

fn read_vdf(fs: &dyn Fs, path: &Path) -> Option<Result<VdfObject, vdf::VdfError>> {
    let bytes = match fs.read(path, vdf::VDF_MAX_BYTES as u64) {
        Ok(b) => b,
        Err(e) if matches!(e.kind, crate::fsx::FsErrorKind::TooLarge { .. }) => {
            return Some(Err(vdf::VdfError::TooLarge));
        }
        Err(_) => return None,
    };
    Some(vdf::parse_bytes(&bytes))
}

/// The display name from `compatibilitytool.vdf`, when present.
pub fn display_name_from(compat: &VdfObject) -> Option<String> {
    let tools = compat
        .get_obj("compatibilitytools")?
        .get_obj("compat_tools")?;
    let (_, tool) = tools.objects().next()?;
    tool.get_str("display_name")
        .map(|s| s.chars().filter(|c| !c.is_control()).take(96).collect())
}

/// Whether a path is on discovery's deny-list (Flatpak application data, rule 5).
pub fn is_denied(path: &Path, home: Option<&Path>) -> bool {
    home.is_some_and(|h| path.starts_with(h.join(".var/app")))
        || path.starts_with("/var/lib/flatpak")
}

/// Validate one candidate directory (rules 1–5).
pub fn validate_build(
    fs: &dyn Fs,
    dir: &Path,
    origin: RuntimeOrigin,
    label: &str,
    home: Option<&Path>,
    now: Timestamp,
) -> Result<ProtonBuild, RejectReason> {
    if is_denied(dir, home) {
        return Err(RejectReason::Denied);
    }
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| RuntimeName::new(n).ok().or_else(|| name_of_folder(n)))
        .ok_or(RejectReason::InvalidName)?;
    let manifest = read_vdf(fs, &dir.join("toolmanifest.vdf"))
        .ok_or(RejectReason::NoManifest)?
        .map_err(RejectReason::BadManifest)?;
    let m = manifest
        .get_obj("manifest")
        .ok_or(RejectReason::NotProton {
            layer: None,
            commandline: None,
        })?;
    let layer = m.get_str("compatmanager_layer_name");
    let commandline = m.get_str("commandline");
    if layer != Some("proton")
        || !commandline.is_some_and(|c| c.trim_start().starts_with("/proton"))
    {
        return Err(RejectReason::NotProton {
            layer: layer.map(str::to_owned),
            commandline: commandline.map(str::to_owned),
        });
    }
    if !fs.metadata(&dir.join("proton")).is_ok_and(|m| m.is_file()) {
        return Err(RejectReason::NoProtonScript);
    }
    let appid = m
        .get_str("require_tool_appid")
        .ok_or(RejectReason::HostRuntime)?;
    let required_slr =
        slr_for_appid(appid).ok_or_else(|| RejectReason::UnsupportedRuntime(appid.to_owned()))?;
    let display_name = read_vdf(fs, &dir.join("compatibilitytool.vdf"))
        .and_then(Result::ok)
        .and_then(|c| display_name_from(&c))
        .unwrap_or_else(|| super::store::pretty_name(name.as_str()));
    let has_fixes = fs.exists(&dir.join("protonfixes/umu-database.csv"));
    let family = family_of(name.as_str());
    Ok(RuntimeBuild {
        version_key: version_key(name.as_str()),
        family,
        origin,
        origin_label: label.to_owned(),
        path: dir.to_path_buf(),
        required_slr,
        display_name,
        has_fixes: has_fixes && family != RuntimeFamily::ValveProton,
        size_bytes: None,
        discovered_at: now,
        last_used_at: None,
        name,
    })
}

/// A build name for a folder whose name is not one (Valve's own builds live in folders such
/// as `Proton - Experimental` and `Proton 9.0`, ADR-0031 §6): every run of characters
/// outside `[A-Za-z0-9._+-]` becomes one `-`; `None` when nothing usable is left. The folder
/// itself is used as it is.
pub fn name_of_folder(folder: &str) -> Option<RuntimeName> {
    let mut out = String::new();
    for c in folder.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut s = out.trim_matches('-').to_owned();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.truncate(96);
    RuntimeName::new(s).ok()
}

/// Detect an SLR variant directory (marker, `umu` link, platform directory, version).
pub fn detect_slr(fs: &dyn Fs, dir: &Path, variant: SlrVariant) -> Option<SlrInstall> {
    if !fs.metadata(dir).is_ok_and(|m| m.is_dir()) {
        return None;
    }
    let marker = fs
        .metadata(&dir.join(".installed.ok"))
        .is_ok_and(|m| m.is_file());
    let umu_link = fs
        .metadata(&dir.join("umu"))
        .is_ok_and(|m| m.file_type == FileType::Symlink);
    let platform_dir = fs.read_dir(dir).ok().and_then(|entries| {
        entries
            .into_iter()
            .filter(|e| e.file_type == FileType::Dir)
            .find(|e| e.name.to_str().is_some_and(|n| n.contains("_platform_")))
            .map(|e| e.path)
    });
    let version = fs
        .read(&dir.join("VERSIONS.txt"), SMALL_FILE)
        .ok()
        .and_then(|b| parse_versions_txt(&String::from_utf8_lossy(&b)))
        .or_else(|| {
            platform_dir.as_ref().and_then(|p| {
                p.file_name()?
                    .to_str()?
                    .split("_platform_")
                    .nth(1)
                    .map(str::to_owned)
            })
        });
    Some(SlrInstall {
        variant,
        path: dir.to_path_buf(),
        version,
        platform_dir,
        marker,
        umu_link,
    })
}

/// The `depot` version of `VERSIONS.txt`.
pub fn parse_versions_txt(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut f = l.split('\t').filter(|s| !s.is_empty());
        (f.next()? == "depot").then(|| f.next().map(str::to_owned))?
    })
}

fn scan_location(
    fs: &dyn Fs,
    loc: &Location,
    input: &DiscoveryInput<'_>,
    seen_paths: &mut Vec<PathBuf>,
    out: &mut Discovery,
) {
    let Ok(root) = fs.canonicalize(&loc.dir) else {
        return;
    };
    if is_denied(&root, input.home) {
        return;
    }
    let Ok(entries) = fs.read_dir(&root) else {
        return;
    };
    for e in entries.into_iter().take(MAX_ENTRIES) {
        if e.name.to_str().is_some_and(|n| n.starts_with('.')) {
            continue;
        }
        let path = match e.file_type {
            FileType::Dir => e.path.clone(),
            FileType::Symlink => match fs.canonicalize(&e.path) {
                Ok(t) if t.starts_with(&root) && t != root => t,
                Ok(_) => {
                    out.rejected.push(Rejected {
                        path: e.path.clone(),
                        reason: RejectReason::SymlinkOutside,
                    });
                    continue;
                }
                Err(_) => continue,
            },
            _ => continue,
        };
        if !fs.metadata(&path).is_ok_and(|m| m.is_dir()) || seen_paths.contains(&path) {
            continue;
        }
        seen_paths.push(path.clone());
        match validate_build(fs, &path, loc.origin, loc.label, input.home, input.now) {
            Ok(b) if out.builds.iter().any(|x| x.name == b.name) => out.rejected.push(Rejected {
                path,
                reason: RejectReason::Duplicate,
            }),
            Ok(b) => out.builds.push(b),
            Err(reason) => out.rejected.push(Rejected { path, reason }),
        }
    }
}

const VARIANTS: [SlrVariant; 2] = [SlrVariant::Steamrt3, SlrVariant::Steamrt4];

/// Scan everything.
pub fn discover(fs: &dyn Fs, input: &DiscoveryInput<'_>) -> Discovery {
    let mut out = Discovery::default();
    let mut seen = Vec::new();
    for loc in locations(input.store, input.home, input.extra_dirs) {
        scan_location(fs, &loc, input, &mut seen, &mut out);
    }
    out.slr = VARIANTS
        .iter()
        .filter_map(|v| detect_slr(fs, &input.store.slr_dir(*v), *v))
        .collect();
    if let Some(home) = input.home {
        let umu = home.join(".local/share/umu");
        if fs.canonicalize(&umu).ok() != fs.canonicalize(&input.store.umu).ok() {
            out.foreign.slr = VARIANTS
                .iter()
                .filter_map(|v| detect_slr(fs, &umu.join(v.as_str()), *v))
                .collect();
        }
        out.foreign.partial_downloads = fs
            .read_dir(&home.join(".cache/umu"))
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|e| e.name.to_str().is_some_and(|n| n.ends_with(".parts")))
                    .map(|e| e.path)
                    .collect()
            })
            .unwrap_or_default();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appids() {
        assert_eq!(slr_for_appid("1628350"), Some(SlrVariant::Steamrt3));
        assert_eq!(slr_for_appid("4183110"), Some(SlrVariant::Steamrt4));
        assert_eq!(slr_for_appid("1391110"), None);
    }

    #[test]
    fn versions_txt() {
        let t = include_str!("../../tests/data/vdf/steamrt3.VERSIONS.txt");
        assert_eq!(
            parse_versions_txt(t).as_deref(),
            Some("3.0.20261110.268881")
        );
        assert_eq!(parse_versions_txt("x\ty\n"), None);
    }
}
