//! The deny-list of SECURITY_MODEL §7.2.
//!
//! Relations are evaluated component-wise on canonical, lexically normalised paths, never by
//! string prefix (`/home/sam/.sshkeys` is not inside `/home/sam/.ssh`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsx::MountTable;
use crate::paths::{AppPaths, normalize};

/// Deny-list group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyGroup {
    /// `/`, `/home`, `$HOME`, … (equal).
    TopLevel,
    /// `/etc`, `/usr`, … (equal or inside).
    System,
    /// Credentials (equal, inside or ancestor).
    Credentials,
    /// Browser profiles.
    Browsers,
    /// Shell start-up files and desktop control.
    Shell,
    /// EasyPlay's own directories.
    EasyPlay,
    /// Container and VM control sockets.
    Containers,
}

impl DenyGroup {
    /// Whether Developer Mode may expose an entry of this group through an
    /// `ExtraReadOnlyPath` exception.
    pub const fn bypassable(self) -> bool {
        matches!(
            self,
            DenyGroup::Credentials | DenyGroup::Browsers | DenyGroup::Shell
        )
    }

    /// Whether `relation` is denied for this group.
    pub const fn denies(self, relation: Relation) -> bool {
        match self {
            DenyGroup::TopLevel => matches!(relation, Relation::Equal),
            DenyGroup::System => matches!(relation, Relation::Equal | Relation::Inside),
            _ => true,
        }
    }
}

/// How a candidate relates to an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// The candidate is the entry.
    Equal,
    /// The candidate is below the entry.
    Inside,
    /// The entry is below the candidate.
    Ancestor,
}

/// Relation of `candidate` to `entry`, if any.
pub fn relation(candidate: &Path, entry: &Path) -> Option<Relation> {
    if candidate == entry {
        Some(Relation::Equal)
    } else if candidate.starts_with(entry) {
        Some(Relation::Inside)
    } else if entry.starts_with(candidate) {
        Some(Relation::Ancestor)
    } else {
        None
    }
}

/// One deny-list match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenyHit {
    /// Group of the entry.
    pub group: DenyGroup,
    /// The entry.
    pub entry: PathBuf,
    /// The relation that matched.
    pub relation: Relation,
}

/// The deny-list resolved for one user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenyList {
    entries: Vec<(DenyGroup, PathBuf)>,
}

const CREDENTIALS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".pki",
    ".password-store",
    ".local/share/keyrings",
    ".config/keepassxc",
    ".aws",
    ".azure",
    ".config/gcloud",
    ".kube",
    ".docker",
    ".netrc",
    ".git-credentials",
    ".config/gh",
    ".config/op",
    ".config/Bitwarden",
    ".mozilla",
    ".thunderbird",
];

const BROWSERS: &[&str] = &[
    ".config/google-chrome",
    ".config/chromium",
    ".config/BraveSoftware",
    ".config/vivaldi",
    ".config/microsoft-edge",
    ".config/opera",
    ".librewolf",
    ".zen",
    ".var/app",
];

const SHELL: &[&str] = &[
    ".bashrc",
    ".profile",
    ".zshrc",
    ".config/fish",
    ".config/autostart",
    ".config/systemd",
    ".local/share/applications",
    ".local/bin",
    ".config/environment.d",
];

const CONTAINERS: &[&str] = &[
    "/var/run/docker.sock",
    "/run/docker.sock",
    "/run/podman",
    "/var/lib/libvirt",
];

impl DenyList {
    /// Build the list for a user. `mounts` contributes "any mount root that is also an
    /// ancestor of `$HOME`".
    pub fn new(
        home: &Path,
        uid: u32,
        user_name: &str,
        paths: &AppPaths,
        mounts: Option<&MountTable>,
    ) -> Self {
        let home = normalize(home);
        let mut entries: Vec<(DenyGroup, PathBuf)> = Vec::new();
        let mut push = |g: DenyGroup, p: PathBuf| entries.push((g, normalize(&p)));
        for p in [
            "/",
            "/home",
            "/var",
            "/run",
            "/tmp",
            "/mnt",
            "/media",
            "/run/media",
        ] {
            push(DenyGroup::TopLevel, PathBuf::from(p));
        }
        push(DenyGroup::TopLevel, home.clone());
        push(DenyGroup::TopLevel, Path::new("/run/media").join(user_name));
        push(DenyGroup::TopLevel, Path::new("/media").join(user_name));
        if let Some(table) = mounts {
            for m in &table.mounts {
                if home.starts_with(&m.mount_point) && m.mount_point != home {
                    push(DenyGroup::TopLevel, m.mount_point.clone());
                }
            }
        }
        for p in [
            "/etc", "/usr", "/boot", "/root", "/proc", "/sys", "/dev", "/nix", "/gnu",
        ] {
            push(DenyGroup::System, PathBuf::from(p));
        }
        push(DenyGroup::System, PathBuf::from(format!("/run/user/{uid}")));
        for rel in CREDENTIALS {
            push(DenyGroup::Credentials, home.join(rel));
        }
        for rel in BROWSERS {
            push(DenyGroup::Browsers, home.join(rel));
        }
        for rel in SHELL {
            push(DenyGroup::Shell, home.join(rel));
        }
        for dir in [
            &paths.config_dir,
            &paths.data_dir,
            &paths.cache_dir,
            &paths.state_dir,
        ] {
            push(DenyGroup::EasyPlay, dir.clone());
        }
        for p in CONTAINERS {
            push(DenyGroup::Containers, PathBuf::from(p));
        }
        entries.dedup();
        Self { entries }
    }

    /// Every entry.
    pub fn entries(&self) -> &[(DenyGroup, PathBuf)] {
        &self.entries
    }

    /// All matches for a canonical candidate, most severe (non-bypassable) first.
    pub fn check(&self, candidate: &Path) -> Vec<DenyHit> {
        let candidate = normalize(candidate);
        let mut hits: Vec<DenyHit> = self
            .entries
            .iter()
            .filter_map(|(group, entry)| {
                let rel = relation(&candidate, entry)?;
                group.denies(rel).then(|| DenyHit {
                    group: *group,
                    entry: entry.clone(),
                    relation: rel,
                })
            })
            .collect();
        hits.sort_by_key(|h| (h.group.bypassable(), h.group));
        hits
    }

    /// Whether the candidate is denied at all.
    pub fn is_denied(&self, candidate: &Path) -> bool {
        !self.check(candidate).is_empty()
    }
}
