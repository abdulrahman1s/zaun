//! Shared and sensitive directories: the "loose executable" rule (GAME_IDENTITY §5.1).
//!
//! A directory is *shared* when making it a game's root would expose things that are not
//! the game's (the home directory, Downloads, a drive root, another game's folder …). An
//! executable whose folder is shared becomes a loose executable with file-only access.
//! The predicate only ever makes setup stricter.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::fsx::{Fs, MountTable};
use crate::paths::{AppPaths, PathEnv, normalize};

/// Largest `user-dirs.dirs` read.
const USER_DIRS_MAX_BYTES: u64 = 16 * 1024;

/// The XDG user directories and their English fallback names.
pub const XDG_USER_DIRS: &[(&str, &str)] = &[
    ("XDG_DESKTOP_DIR", "Desktop"),
    ("XDG_DOWNLOAD_DIR", "Downloads"),
    ("XDG_DOCUMENTS_DIR", "Documents"),
    ("XDG_MUSIC_DIR", "Music"),
    ("XDG_PICTURES_DIR", "Pictures"),
    ("XDG_VIDEOS_DIR", "Videos"),
    ("XDG_TEMPLATES_DIR", "Templates"),
    ("XDG_PUBLICSHARE_DIR", "Public"),
];

/// System trees that are never a game folder (SECURITY_MODEL §7.2 "System", equal or inside).
const SYSTEM_TREES: &[&str] = &[
    "/etc", "/usr", "/boot", "/root", "/proc", "/sys", "/dev", "/nix", "/gnu", "/bin", "/sbin",
    "/lib", "/lib64",
];

/// Why a directory is shared.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SharedReason {
    /// The home directory.
    Home,
    /// An ancestor of the home directory (`/home`, `/`).
    HomeAncestor,
    /// An XDG user directory; the English name (`Downloads`, …).
    UserDir(String),
    /// `/tmp`, `/var/tmp`, `/dev/shm` or `$XDG_RUNTIME_DIR`.
    Temporary,
    /// The root of a mounted filesystem.
    MountRoot,
    /// `/mnt`, `/media`, `/run/media/<user>` and similar parents of mount points.
    MediaParent,
    /// Inside a system tree (`/usr`, `/etc`, …).
    System,
    /// On the deny-list of SECURITY_MODEL §7.2 (setup refuses the folder outright).
    Denied,
    /// Another game's root.
    OtherGameRoot,
    /// One of EasyPlay's own directories, or inside one.
    EasyPlay,
}

/// Deny-list predicate supplied by `sandbox::validate` (equal/inside/ancestor relations of
/// SECURITY_MODEL §7.2 on canonical paths).
pub type DenyPredicate = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// The facts the predicate needs, gathered once per setup.
#[derive(Clone, Default)]
pub struct SharedLocations {
    /// `$HOME`.
    pub home: Option<PathBuf>,
    /// XDG user directories with their English names.
    pub user_dirs: Vec<(String, PathBuf)>,
    /// Temporary directories.
    pub temp_dirs: Vec<PathBuf>,
    /// Mount points (`fsx::MountTable`).
    pub mount_points: Vec<PathBuf>,
    /// Parents of removable-media mount points.
    pub media_parents: Vec<PathBuf>,
    /// EasyPlay's configuration, data, cache and state directories.
    pub easyplay_dirs: Vec<PathBuf>,
    /// Roots of games already in the library.
    pub game_roots: Vec<PathBuf>,
    /// The deny-list, when available.
    pub deny: Option<DenyPredicate>,
}

impl std::fmt::Debug for SharedLocations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedLocations")
            .field("home", &self.home)
            .field("user_dirs", &self.user_dirs)
            .field("temp_dirs", &self.temp_dirs)
            .field("mount_points", &self.mount_points)
            .field("media_parents", &self.media_parents)
            .field("easyplay_dirs", &self.easyplay_dirs)
            .field("game_roots", &self.game_roots)
            .field("deny", &self.deny.is_some())
            .finish()
    }
}

/// Parse `user-dirs.dirs`: `XDG_<NAME>_DIR="$HOME/…"` or an absolute path. Entries equal to
/// `$HOME` (the XDG way of disabling a directory) are dropped.
pub fn parse_user_dirs(content: &str, home: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for line in content.lines().take(256) {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(&(_, english)) = XDG_USER_DIRS.iter().find(|(k, _)| *k == key.trim()) else {
            continue;
        };
        let v = value.trim().trim_matches('"');
        let path = if let Some(rest) = v.strip_prefix("$HOME") {
            let rest = rest.trim_start_matches('/');
            if rest.is_empty() {
                continue;
            }
            home.join(rest)
        } else if v.starts_with('/') {
            PathBuf::from(v)
        } else {
            continue;
        };
        let path = normalize(&path);
        if path != home {
            out.push((english.to_owned(), path));
        }
    }
    out
}

fn equal_or_inside(dir: &Path, tree: &Path) -> bool {
    dir.starts_with(tree)
}

impl SharedLocations {
    /// Gather the facts from the environment, `user-dirs.dirs` (read through `fs`), the
    /// mount table and EasyPlay's paths. `game_roots` and `deny` are filled by the caller.
    pub fn gather(fs: &dyn Fs, env: &PathEnv, paths: &AppPaths, mounts: &MountTable) -> Self {
        let home = paths.home.clone();
        let mut user_dirs = Vec::new();
        if let Some(h) = &home {
            let cfg = env
                .get("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| h.join(".config"));
            if let Ok(bytes) = fs.read(&cfg.join("user-dirs.dirs"), USER_DIRS_MAX_BYTES) {
                user_dirs = parse_user_dirs(&String::from_utf8_lossy(&bytes), h);
            }
            for (_, english) in XDG_USER_DIRS {
                let p = h.join(english);
                if !user_dirs.iter().any(|(_, q)| *q == p) {
                    user_dirs.push(((*english).to_owned(), p));
                }
            }
        }
        let mut temp_dirs: Vec<PathBuf> = ["/tmp", "/var/tmp", "/dev/shm"]
            .iter()
            .map(PathBuf::from)
            .collect();
        if let Some(x) = &paths.xdg_runtime_dir {
            temp_dirs.push(x.clone());
        }
        let user = env
            .get("USER")
            .map(|u| u.to_string_lossy().into_owned())
            .filter(|u| !u.is_empty() && !u.contains('/'));
        let mut media_parents: Vec<PathBuf> = ["/mnt", "/media", "/run/media"]
            .iter()
            .map(PathBuf::from)
            .collect();
        if let Some(u) = user {
            media_parents.push(Path::new("/run/media").join(&u));
            media_parents.push(Path::new("/media").join(&u));
        }
        Self {
            home,
            user_dirs,
            temp_dirs,
            mount_points: mounts
                .mounts
                .iter()
                .map(|m| m.mount_point.clone())
                .collect(),
            media_parents,
            easyplay_dirs: vec![
                paths.config_dir.clone(),
                paths.data_dir.clone(),
                paths.cache_dir.clone(),
                paths.state_dir.clone(),
            ],
            game_roots: Vec::new(),
            deny: None,
        }
    }

    /// Whether `dir` (canonical) is the root of a mounted filesystem.
    pub fn is_mount_root(&self, dir: &Path) -> bool {
        dir == Path::new("/") || self.mount_points.iter().any(|m| m == dir)
    }

    /// Why `dir` (canonical, absolute) is shared, or `None` when it may be a game folder.
    pub fn classify(&self, dir: &Path) -> Option<SharedReason> {
        let dir = normalize(dir);
        let dir = dir.as_path();
        if let Some(d) = &self.deny
            && d(dir)
        {
            return Some(SharedReason::Denied);
        }
        if SYSTEM_TREES
            .iter()
            .any(|t| equal_or_inside(dir, Path::new(t)))
        {
            return Some(SharedReason::System);
        }
        if self.easyplay_dirs.iter().any(|e| equal_or_inside(dir, e)) {
            return Some(SharedReason::EasyPlay);
        }
        if let Some(h) = &self.home {
            if dir == h {
                return Some(SharedReason::Home);
            }
            if h.starts_with(dir) {
                return Some(SharedReason::HomeAncestor);
            }
        }
        if dir == Path::new("/") {
            return Some(SharedReason::MountRoot);
        }
        if let Some((name, _)) = self.user_dirs.iter().find(|(_, p)| p == dir) {
            return Some(SharedReason::UserDir(name.clone()));
        }
        if self.temp_dirs.iter().any(|t| t == dir) {
            return Some(SharedReason::Temporary);
        }
        if self.media_parents.iter().any(|m| m == dir) {
            return Some(SharedReason::MediaParent);
        }
        if self.is_mount_root(dir) {
            return Some(SharedReason::MountRoot);
        }
        if self.game_roots.iter().any(|r| r == dir) {
            return Some(SharedReason::OtherGameRoot);
        }
        None
    }

    /// `sensitive::is_shared_location` of GAME_IDENTITY §5.1.
    pub fn is_shared_location(&self, dir: &Path) -> bool {
        self.classify(dir).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locs() -> SharedLocations {
        SharedLocations {
            home: Some("/home/sam".into()),
            user_dirs: vec![
                ("Downloads".into(), "/home/sam/Téléchargements".into()),
                ("Desktop".into(), "/home/sam/Desktop".into()),
            ],
            temp_dirs: vec!["/tmp".into(), "/run/user/1000".into()],
            mount_points: vec!["/".into(), "/run/media/sam/Games".into(), "/home".into()],
            media_parents: vec!["/mnt".into(), "/run/media/sam".into()],
            easyplay_dirs: vec!["/home/sam/.local/share/easyplay".into()],
            game_roots: vec!["/home/sam/Games/Celeste".into()],
            deny: Some(Arc::new(|p: &Path| p.starts_with("/home/sam/.ssh"))),
        }
    }

    #[test]
    fn classification_table() {
        let l = locs();
        for (dir, want) in [
            ("/home/sam", Some(SharedReason::Home)),
            ("/home", Some(SharedReason::HomeAncestor)),
            ("/", Some(SharedReason::HomeAncestor)),
            (
                "/home/sam/Téléchargements",
                Some(SharedReason::UserDir("Downloads".into())),
            ),
            ("/tmp", Some(SharedReason::Temporary)),
            ("/run/user/1000", Some(SharedReason::Temporary)),
            ("/run/media/sam/Games", Some(SharedReason::MountRoot)),
            ("/run/media/sam", Some(SharedReason::MediaParent)),
            ("/mnt", Some(SharedReason::MediaParent)),
            ("/usr/bin", Some(SharedReason::System)),
            ("/home/sam/.ssh", Some(SharedReason::Denied)),
            (
                "/home/sam/.local/share/easyplay/games/x",
                Some(SharedReason::EasyPlay),
            ),
            ("/home/sam/Games/Celeste", Some(SharedReason::OtherGameRoot)),
            ("/home/sam/Games/Celeste/sub", None),
            ("/home/sam/Games", None),
            ("/home/sam/Downloads/Game", None),
            ("/run/media/sam/Games/Hollow Knight", None),
            ("/tmp/x", None),
        ] {
            assert_eq!(l.classify(Path::new(dir)), want, "{dir}");
        }
    }

    #[test]
    fn user_dirs_file() {
        let content = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Bureau\"\nXDG_DOWNLOAD_DIR=\"$HOME/\"\n\
                       XDG_MUSIC_DIR=\"/data/music\"\nXDG_VIDEOS_DIR=relative\nBOGUS=1\n";
        let got = parse_user_dirs(content, Path::new("/home/sam"));
        assert_eq!(
            got,
            vec![
                ("Desktop".to_owned(), PathBuf::from("/home/sam/Bureau")),
                ("Music".to_owned(), PathBuf::from("/data/music")),
            ]
        );
    }
}
