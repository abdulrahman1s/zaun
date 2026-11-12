//! Runtime store layout and build naming (RUNTIME_MANAGEMENT §2–3).
//!
//! [`RuntimeStore`] names every host path of the store; [`SandboxStore`] names the same
//! locations as they appear inside a sandbox (umu's default paths under the real home, so no
//! relocation variable is needed: `UMU`, `COMPAT`, `CACHE`, `RT`, `PL` of
//! BUBBLEWRAP_SANDBOX §4.9). Build names are parsed into a family and a zero-padded natural
//! version key so that `GE-Proton10-25` sorts after `GE-Proton10-9`.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::clock::Timestamp;
use crate::ids::SessionId;
use crate::model::{RuntimeFamily, SlrVariant};
use crate::paths::AppPaths;

/// Host-side paths of the runtime store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeStore {
    /// `STORE = <data>/runtimes`.
    pub root: PathBuf,
    /// `STORE/umu` (mounted at `UMU`).
    pub umu: PathBuf,
    /// `STORE/Steam/compatibilitytools.d` (mounted at `COMPAT`).
    pub compat: PathBuf,
    /// `STORE/session-var`.
    pub session_var: PathBuf,
    /// `STORE/quarantine`.
    pub quarantine: PathBuf,
    /// `<data>/runtime-manifests` (never mounted).
    pub manifests: PathBuf,
    /// `<cache>/umu` (mounted at `CACHE` in trusted network sessions).
    pub cache: PathBuf,
}

impl RuntimeStore {
    /// The store of `paths`.
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            root: paths.runtimes_dir.clone(),
            umu: paths.umu_store_dir.clone(),
            compat: paths.compat_tools_dir.clone(),
            session_var: paths.session_var_root.clone(),
            quarantine: paths.quarantine_dir.clone(),
            manifests: paths.runtime_manifests_dir.clone(),
            cache: paths.umu_cache_dir.clone(),
        }
    }

    /// A store rooted at `data` with umu's cache at `cache` (tests).
    pub fn at(data: &Path, cache: &Path) -> Self {
        let root = data.join("runtimes");
        Self {
            umu: root.join("umu"),
            compat: root.join("Steam/compatibilitytools.d"),
            session_var: root.join("session-var"),
            quarantine: root.join("quarantine"),
            manifests: data.join("runtime-manifests"),
            cache: cache.join("umu"),
            root,
        }
    }

    /// `STORE/umu/<variant>`.
    pub fn slr_dir(&self, variant: SlrVariant) -> PathBuf {
        self.umu.join(variant.as_str())
    }

    /// `STORE/umu/umu-shim`.
    pub fn umu_shim(&self) -> PathBuf {
        self.umu.join("umu-shim")
    }

    /// `STORE/umu/umu.lock`.
    pub fn umu_lock(&self) -> PathBuf {
        self.umu.join("umu.lock")
    }

    /// `STORE/Steam/compatibilitytools.d/<name>`.
    pub fn proton_dir(&self, name: &str) -> PathBuf {
        self.compat.join(name)
    }

    /// `STORE/session-var/<sid>`.
    pub fn session_var_dir(&self, sid: SessionId) -> PathBuf {
        self.session_var.join(sid.to_string())
    }

    /// `STORE/quarantine/<variant>-<timestamp>`.
    pub fn quarantine_dir(&self, variant: SlrVariant, at: Timestamp) -> PathBuf {
        self.quarantine
            .join(format!("{}-{}", variant.as_str(), at.as_millis()))
    }

    /// `<data>/runtime-manifests/<variant>.manifest.json`.
    pub fn manifest_file(&self, variant: SlrVariant) -> PathBuf {
        self.manifests
            .join(format!("{}.manifest.json", variant.as_str()))
    }
}

/// The store as seen inside a sandbox whose `HOME` is `home`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxStore {
    /// `UMU = <home>/.local/share/umu`.
    pub umu: PathBuf,
    /// `COMPAT = <home>/.local/share/Steam/compatibilitytools.d`.
    pub compat: PathBuf,
    /// `CACHE = <home>/.cache/umu`.
    pub cache: PathBuf,
}

impl SandboxStore {
    /// Locations for the real home path `home`.
    pub fn new(home: &Path) -> Self {
        Self {
            umu: home.join(".local/share/umu"),
            compat: home.join(".local/share/Steam/compatibilitytools.d"),
            cache: home.join(".cache/umu"),
        }
    }

    /// `RT = UMU/<variant>`.
    pub fn rt(&self, variant: SlrVariant) -> PathBuf {
        self.umu.join(variant.as_str())
    }

    /// `PL = RT/<platform dir name>`.
    pub fn pl(&self, variant: SlrVariant, platform_dir_name: &str) -> PathBuf {
        self.rt(variant).join(platform_dir_name)
    }

    /// `COMPAT/<name>`.
    pub fn proton_dir(&self, name: &str) -> PathBuf {
        self.compat.join(name)
    }
}

// ---- naming ---------------------------------------------------------------------------------

fn digits_only(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `GE-ProtonM-N` → `[M, N]`.
fn ge_parts(name: &str) -> Option<Vec<u64>> {
    let rest = name.strip_prefix("GE-Proton")?;
    let (m, n) = rest.split_once('-')?;
    if !(digits_only(m) && digits_only(n)) {
        return None;
    }
    Some(vec![m.parse().ok()?, n.parse().ok()?])
}

/// `UMU-Proton-X.Y-Z` (or `UMU-Proton-X.Y`) → `[X, Y, Z]`.
fn umu_parts(name: &str) -> Option<Vec<u64>> {
    let rest = name.strip_prefix("UMU-Proton-")?;
    let (xy, z) = match rest.split_once('-') {
        Some((xy, z)) => (xy, Some(z)),
        None => (rest, None),
    };
    let (x, y) = xy.split_once('.')?;
    if !digits_only(x) || !digits_only(y) || z.is_some_and(|z| !digits_only(z)) {
        return None;
    }
    let mut v = vec![x.parse().ok()?, y.parse().ok()?];
    if let Some(z) = z {
        v.push(z.parse().ok()?);
    }
    Some(v)
}

/// Every run of digits in `name`.
fn all_numbers(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.get(..s.len().min(18)).and_then(|t| t.parse().ok()))
        .collect()
}

/// The family of a build from its directory name (RUNTIME_MANAGEMENT §5 rule 4).
pub fn family_of(name: &str) -> RuntimeFamily {
    if ge_parts(name).is_some() {
        RuntimeFamily::GeProton
    } else if umu_parts(name).is_some() {
        RuntimeFamily::UmuProton
    } else if name.starts_with("Proton") || name.starts_with("proton_") {
        RuntimeFamily::ValveProton
    } else {
        RuntimeFamily::Other
    }
}

/// Zero-padded natural sort key: each number as eight digits, joined with `.`.
pub fn version_key(name: &str) -> String {
    let nums = ge_parts(name)
        .or_else(|| umu_parts(name))
        .unwrap_or_else(|| all_numbers(name));
    nums.iter()
        .map(|n| format!("{:08}", n.min(&99_999_999)))
        .collect::<Vec<_>>()
        .join(".")
}

/// Natural order of two build names of the same family: by version key, then by name.
pub fn compare_builds(a: &str, b: &str) -> Ordering {
    version_key(a).cmp(&version_key(b)).then_with(|| a.cmp(b))
}

/// Whether a pinned name belongs to a family umu can download (RUNTIME_MANAGEMENT §3).
pub fn is_downloadable_name(name: &str) -> bool {
    matches!(
        family_of(name),
        RuntimeFamily::GeProton | RuntimeFamily::UmuProton
    )
}

/// Human display of a build name ("GE-Proton 10-25").
pub fn pretty_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("GE-Proton")
        && ge_parts(name).is_some()
    {
        return format!("GE-Proton {rest}");
    }
    if let Some(rest) = name.strip_prefix("UMU-Proton-")
        && umu_parts(name).is_some()
    {
        return format!("UMU-Proton {rest}");
    }
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering_table() {
        // (lower, higher)
        let cases = [
            ("GE-Proton10-9", "GE-Proton10-25"),
            ("GE-Proton9-27", "GE-Proton10-1"),
            ("GE-Proton10-25", "GE-Proton10-26"),
            ("UMU-Proton-9.0-4", "UMU-Proton-10.0-4"),
            ("UMU-Proton-10.0-3", "UMU-Proton-10.0-4"),
            ("UMU-Proton-10.0", "UMU-Proton-10.0-1"),
            ("UMU-Proton-10.0-4", "UMU-Proton-10.1-1"),
            (
                "proton-cachyos-10.0-20250101",
                "proton-cachyos-11.0-20260703-slr-x86_64_v3",
            ),
        ];
        for (lo, hi) in cases {
            assert_eq!(compare_builds(lo, hi), Ordering::Less, "{lo} < {hi}");
            assert_eq!(compare_builds(hi, lo), Ordering::Greater, "{hi} > {lo}");
        }
        assert_eq!(
            compare_builds("GE-Proton10-25", "GE-Proton10-25"),
            Ordering::Equal
        );
    }

    #[test]
    fn families_and_keys() {
        assert_eq!(family_of("GE-Proton10-25"), RuntimeFamily::GeProton);
        assert_eq!(family_of("UMU-Proton-10.0-4"), RuntimeFamily::UmuProton);
        assert_eq!(family_of("Proton 9.0"), RuntimeFamily::ValveProton);
        assert_eq!(family_of("GE-Proton10-25-rtsp"), RuntimeFamily::Other);
        assert_eq!(
            family_of("proton-cachyos-11.0-20260703-slr-x86_64_v3"),
            RuntimeFamily::Other
        );
        assert_eq!(version_key("GE-Proton10-25"), "00000010.00000025");
        assert_eq!(
            version_key("UMU-Proton-10.0-4"),
            "00000010.00000000.00000004"
        );
        assert!(is_downloadable_name("GE-Proton9-1") && !is_downloadable_name("custom"));
        assert_eq!(pretty_name("GE-Proton10-25"), "GE-Proton 10-25");
        assert_eq!(pretty_name("UMU-Proton-10.0-4"), "UMU-Proton 10.0-4");
    }

    #[test]
    fn paths() {
        let s = RuntimeStore::at(Path::new("/d"), Path::new("/c"));
        assert_eq!(
            s.slr_dir(SlrVariant::Steamrt3),
            Path::new("/d/runtimes/umu/steamrt3")
        );
        assert_eq!(
            s.proton_dir("GE-Proton10-25"),
            Path::new("/d/runtimes/Steam/compatibilitytools.d/GE-Proton10-25")
        );
        assert_eq!(
            s.manifest_file(SlrVariant::Steamrt4),
            Path::new("/d/runtime-manifests/steamrt4.manifest.json")
        );
        assert_eq!(s.cache, Path::new("/c/umu"));
        assert_eq!(
            s.quarantine_dir(SlrVariant::Steamrt3, Timestamp::from_millis(5)),
            Path::new("/d/runtimes/quarantine/steamrt3-5")
        );
        let sb = SandboxStore::new(Path::new("/home/sam"));
        assert_eq!(
            sb.rt(SlrVariant::Steamrt3),
            Path::new("/home/sam/.local/share/umu/steamrt3")
        );
        assert_eq!(
            sb.pl(SlrVariant::Steamrt3, "sniper_platform_3.0"),
            Path::new("/home/sam/.local/share/umu/steamrt3/sniper_platform_3.0")
        );
        assert_eq!(
            sb.proton_dir("X"),
            Path::new("/home/sam/.local/share/Steam/compatibilitytools.d/X")
        );
        assert_eq!(sb.cache, Path::new("/home/sam/.cache/umu"));
    }
}
