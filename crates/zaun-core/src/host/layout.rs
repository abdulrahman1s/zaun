//! Filesystem layout probe: merged `/usr`, Nix and Guix, extra library roots, the `/etc`
//! allow-list, the resolver strategy, and the layout-specific environment
//! (BUBBLEWRAP_SANDBOX §4.2–4.3, UMU_INTEGRATION §4 tables A and B).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::facts::{FsLayout, HostEntry, LayoutKind, OsRelease, ResolverStrategy};
use crate::fsx::{FileType, Fs};

/// Top-level entries that may be merged-`/usr` symlinks.
pub const TOP_LEVEL: &[&str] = &["/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32"];

/// `/etc` entries the sandbox may expose (BUBBLEWRAP_SANDBOX §4.3). `passwd`, `group` and
/// `nsswitch.conf` are always synthesized and therefore not probed.
pub const ETC_ALLOW_LIST: &[&str] = &[
    "OpenCL",
    "alsa",
    "alternatives",
    "asound.conf",
    "ca-certificates",
    "ca-certificates.conf",
    "drirc",
    "egl",
    "fonts",
    "gai.conf",
    "glvnd",
    "host.conf",
    "hosts",
    "ld.so.cache",
    "ld.so.conf",
    "ld.so.conf.d",
    "libva.conf",
    "locale.conf",
    "localtime",
    "machine-id",
    "nvidia",
    "openal",
    "os-release",
    "pipewire",
    "pki",
    "pulse",
    "resolv.conf",
    "ssl",
    "timezone",
    "vulkan",
];

/// Variables passed through on Nix and Guix (UMU_INTEGRATION §4 table B, last row).
pub const NIX_PASSTHROUGH_ENV: &[&str] = &[
    "LD_LIBRARY_PATH",
    "LIBGL_DRIVERS_PATH",
    "LIBVA_DRIVER_NAME",
    "VDPAU_DRIVER",
    "GBM_BACKEND",
    "__EGL_VENDOR_LIBRARY_FILENAMES",
    "VK_DRIVER_FILES",
    "VK_ICD_FILENAMES",
    "VK_LAYER_PATH",
    "VK_ADD_LAYER_PATH",
    "LOCALE_ARCHIVE",
    "TZDIR",
    "XCURSOR_PATH",
    "XDG_DATA_DIRS",
    "XDG_CONFIG_DIRS",
    "SSL_CERT_FILE",
    "NIX_SSL_CERT_FILE",
];

/// `PATH` inside the sandbox on an FHS system.
pub const FHS_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
/// `PATH` inside the sandbox on NixOS.
pub const NIX_PATH: &str = "/run/current-system/sw/bin:/usr/bin:/bin";
/// `PATH` inside the sandbox on Guix System.
pub const GUIX_PATH: &str = "/run/current-system/profile/bin:/usr/bin:/bin";

const CONF_CAP: u64 = 64 * 1024;
const MAX_INCLUDES: usize = 64;
const MAX_LIB_ROOTS: usize = 64;

/// Parse `os-release` text (`KEY=value`, optionally quoted).
pub fn parse_os_release(text: &str) -> OsRelease {
    let mut os = OsRelease::default();
    for line in text.lines().take(256) {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value)
            .chars()
            .take(128)
            .collect::<String>();
        match key.trim() {
            "ID" => os.id = Some(value),
            "ID_LIKE" => os.id_like = value.split_whitespace().map(str::to_owned).collect(),
            "VERSION_ID" => os.version_id = Some(value),
            "PRETTY_NAME" => os.pretty_name = Some(value),
            _ => {}
        }
    }
    os
}

/// Read `/etc/os-release`, falling back to `/usr/lib/os-release`.
pub fn read_os_release(fs: &dyn Fs) -> OsRelease {
    ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|p| fs.read_follow(Path::new(p), CONF_CAP).ok())
        .map(|b| parse_os_release(&String::from_utf8_lossy(&b)))
        .unwrap_or_default()
}

/// `lstat` a path into a [`HostEntry`]; `None` when absent.
pub fn host_entry(fs: &dyn Fs, path: &Path) -> Option<HostEntry> {
    let md = fs.metadata(path).ok()?;
    Some(match md.file_type {
        FileType::Dir => HostEntry::Dir,
        FileType::File => HostEntry::File,
        FileType::Symlink => HostEntry::Symlink {
            target: fs.read_link(path).unwrap_or_default(),
            resolved: fs.canonicalize(path).ok(),
        },
        _ => HostEntry::Other,
    })
}

fn is_dir_follow(fs: &dyn Fs, path: &Path) -> bool {
    fs.metadata_follow(path).is_ok_and(|m| m.is_dir())
}

/// Determine the layout family.
pub fn detect_kind(fs: &dyn Fs, os: &OsRelease) -> LayoutKind {
    let current_system = fs.exists(Path::new("/run/current-system"));
    if is_dir_follow(fs, Path::new("/nix/store"))
        && (os.id.as_deref() == Some("nixos") || current_system)
    {
        LayoutKind::Nix
    } else if is_dir_follow(fs, Path::new("/gnu/store"))
        && (os.id.as_deref() == Some("guix") || current_system)
    {
        LayoutKind::Guix
    } else {
        LayoutKind::Fhs
    }
}

/// Expand an `include` pattern: a literal path, or a `*` wildcard in the last component.
fn expand_include(fs: &dyn Fs, base: &Path, pattern: &str) -> Vec<PathBuf> {
    let pattern = if Path::new(pattern).is_absolute() {
        PathBuf::from(pattern)
    } else {
        base.join(pattern)
    };
    let Some(name) = pattern.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    let Some(dir) = pattern.parent() else {
        return Vec::new();
    };
    match name.split_once('*') {
        None => vec![pattern.clone()],
        Some((prefix, suffix)) => fs
            .read_dir(dir)
            .map(|entries| {
                entries
                    .into_iter()
                    .filter_map(|e| {
                        let n = e.name.to_str()?;
                        (n.len() >= prefix.len() + suffix.len()
                            && n.starts_with(prefix)
                            && n.ends_with(suffix))
                        .then_some(e.path)
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Library directories named by `/etc/ld.so.conf`, following `include` lines one level.
pub fn ld_so_conf_dirs(fs: &dyn Fs) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let root = Path::new("/etc/ld.so.conf");
    let mut files = vec![(root.to_path_buf(), true)];
    let mut includes = 0;
    while let Some((file, follow)) = files.pop() {
        let Ok(bytes) = fs.read_follow(&file, CONF_CAP) else {
            continue;
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if let Some(pattern) = line.strip_prefix("include") {
                if !follow || !pattern.starts_with(char::is_whitespace) {
                    continue;
                }
                for p in pattern.split_whitespace() {
                    for inc in expand_include(fs, Path::new("/etc"), p) {
                        if includes < MAX_INCLUDES {
                            includes += 1;
                            files.push((inc, false));
                        }
                    }
                }
            } else if line.starts_with('/') {
                // Entries may be separated by whitespace, commas or colons.
                for dir in line.split(|c: char| c.is_whitespace() || c == ',' || c == ':') {
                    if dir.starts_with('/') && out.len() < MAX_LIB_ROOTS * 4 {
                        out.push(PathBuf::from(dir));
                    }
                }
            }
        }
    }
    out
}

fn covered_by_system_bind(path: &Path) -> bool {
    ["/usr", "/lib", "/lib32", "/lib64", "/libx32", "/opt"]
        .iter()
        .any(|r| path.starts_with(r))
}

/// Inputs of the layout probe besides the filesystem.
#[derive(Clone, Debug, Default)]
pub struct LayoutInput<'a> {
    /// `os-release` already read by the caller.
    pub os: Option<&'a OsRelease>,
    /// The user name (for `/etc/profiles/per-user/<user>` on NixOS).
    pub user_name: Option<&'a str>,
}

/// Probe the filesystem layout.
pub fn probe_layout(fs: &dyn Fs, input: &LayoutInput<'_>) -> FsLayout {
    let owned_os;
    let os = match input.os {
        Some(os) => os,
        None => {
            owned_os = read_os_release(fs);
            &owned_os
        }
    };
    let kind = detect_kind(fs, os);

    let top_level = TOP_LEVEL
        .iter()
        .filter_map(|p| host_entry(fs, Path::new(p)).map(|e| (PathBuf::from(p), e)))
        .collect();

    let mut extra: Vec<PathBuf> = ld_so_conf_dirs(fs)
        .into_iter()
        .filter(|d| !covered_by_system_bind(d))
        .filter_map(|d| fs.canonicalize(&d).ok())
        .filter(|d| !covered_by_system_bind(d) && is_dir_follow(fs, d))
        .filter(|d| match kind {
            LayoutKind::Nix => !d.starts_with("/nix") && !d.starts_with("/run/current-system"),
            LayoutKind::Guix => !d.starts_with("/gnu") && !d.starts_with("/run/current-system"),
            LayoutKind::Fhs => true,
        })
        .collect();
    extra.sort();
    extra.dedup();
    extra.truncate(MAX_LIB_ROOTS);

    let etc: BTreeMap<String, HostEntry> = ETC_ALLOW_LIST
        .iter()
        .filter_map(|n| host_entry(fs, &Path::new("/etc").join(n)).map(|e| ((*n).to_owned(), e)))
        .collect();

    let resolver = match etc.get("resolv.conf") {
        None | Some(HostEntry::Other) | Some(HostEntry::Dir) => ResolverStrategy::Absent,
        Some(HostEntry::File) => ResolverStrategy::File,
        Some(HostEntry::Symlink { target, resolved }) => {
            let abs = resolved
                .clone()
                .unwrap_or_else(|| crate::paths::normalize(&Path::new("/etc").join(target)));
            match abs.parent() {
                Some(dir) if dir != Path::new("/") && dir != Path::new("/etc") => {
                    ResolverStrategy::Symlink {
                        target: target.clone(),
                        bind_dir: dir.to_path_buf(),
                    }
                }
                _ => ResolverStrategy::File,
            }
        }
    };

    let (passthrough_env, path_env) = match kind {
        LayoutKind::Fhs => (Vec::new(), FHS_PATH),
        LayoutKind::Nix => (
            NIX_PASSTHROUGH_ENV
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            NIX_PATH,
        ),
        LayoutKind::Guix => (
            NIX_PASSTHROUGH_ENV
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            GUIX_PATH,
        ),
    };

    let mut trees: Vec<PathBuf> = match kind {
        LayoutKind::Fhs => Vec::new(),
        LayoutKind::Nix => [
            "/run/current-system",
            "/run/opengl-driver",
            "/run/opengl-driver-32",
            "/etc/static",
        ]
        .map(PathBuf::from)
        .to_vec(),
        LayoutKind::Guix => vec![PathBuf::from("/run/current-system")],
    };
    if kind == LayoutKind::Nix
        && let Some(user) = input
            .user_name
            .filter(|u| !u.is_empty() && !u.contains('/'))
    {
        trees.push(Path::new("/etc/profiles/per-user").join(user));
    }
    let system_trees = trees
        .into_iter()
        .filter_map(|p| host_entry(fs, &p).map(|e| (p, e)))
        .collect();

    FsLayout {
        kind,
        top_level,
        has_usr: is_dir_follow(fs, Path::new("/usr")),
        has_opt: fs.exists(Path::new("/opt")),
        extra_lib_roots: extra,
        etc,
        resolver,
        passthrough_env,
        path_env: path_env.to_owned(),
        system_trees,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_parsing() {
        let os = parse_os_release(
            "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\nVERSION_ID=\"24.04\"\n# c\nPRETTY_NAME='Ubuntu 24.04 LTS'\n",
        );
        assert_eq!(os.id.as_deref(), Some("ubuntu"));
        assert_eq!(os.id_like, vec!["debian".to_owned()]);
        assert_eq!(os.version_id.as_deref(), Some("24.04"));
        assert_eq!(os.pretty_name.as_deref(), Some("Ubuntu 24.04 LTS"));
        assert!(os.is_like("debian") && os.is_like("ubuntu") && !os.is_like("arch"));
    }
}
