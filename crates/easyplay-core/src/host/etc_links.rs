//! Resolution of symlinked `/etc` entries.
//!
//! On NixOS most of `/etc` (`zoneinfo`, `nsswitch.conf`, `resolv.conf`, …) is a symlink
//! through `/etc/static`, which is itself a symlink into `/nix/store` and is absent from a
//! freshly built sandbox `/etc`. The sandbox therefore needs the fully resolved target of
//! each entry, not only the link text. Kept separate from [`FsLayout`](super::FsLayout) so
//! that it can be merged with the planner's view of `/etc` independently.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::layout::ETC_ALLOW_LIST;
use crate::fsx::{FileType, Fs};

/// Entries resolved in addition to the allow-list: synthesized by default but needed when a
/// policy binds them, and `zoneinfo`, which the SLR's nested FHS environment requires.
pub const ETC_EXTRA_ENTRIES: &[&str] = &["group", "nsswitch.conf", "passwd", "zoneinfo"];

const MAX_HOPS: usize = 16;

/// One symlinked `/etc` entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtcLink {
    /// Link text of `/etc/<name>`.
    pub target: PathBuf,
    /// `/etc/<name>` followed by every symlink target met while resolving it, including links
    /// in intermediate directories (`/etc/zoneinfo` inside `/etc/localtime`'s target).
    pub chain: Vec<PathBuf>,
    /// Canonical final path; `None` when dangling.
    pub resolved: Option<PathBuf>,
    /// The resolved target is a directory.
    pub is_dir: bool,
    /// The chain passes through `/etc/static` (NixOS), so recreating the link alone does not
    /// work inside a sandbox: bind `resolved` at `/etc/<name>` instead.
    pub via_etc_static: bool,
}

/// Every symlinked entry of [`ETC_ALLOW_LIST`] and [`ETC_EXTRA_ENTRIES`], by name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtcLinkFacts {
    /// Name → resolution. Regular files and directories are not listed.
    pub links: BTreeMap<String, EtcLink>,
}

/// Resolve `start` component by component and return it followed by every symlink target
/// met on the way (including links in intermediate directories), as absolute paths.
fn resolve_chain(fs: &dyn Fs, start: &Path) -> Vec<PathBuf> {
    let mut chain = vec![start.to_path_buf()];
    let mut todo: VecDeque<OsString> = start
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_owned()),
            _ => None,
        })
        .collect();
    let mut cur = PathBuf::from("/");
    let mut hops = 0;
    while let Some(c) = todo.pop_front() {
        let next = cur.join(&c);
        let is_link = fs
            .metadata(&next)
            .is_ok_and(|m| m.file_type == FileType::Symlink);
        if !is_link || hops >= MAX_HOPS {
            cur = next;
            continue;
        }
        hops += 1;
        let Ok(t) = fs.read_link(&next) else {
            cur = next;
            continue;
        };
        let abs = crate::paths::normalize(&cur.join(t));
        let mut rest: VecDeque<OsString> = abs
            .components()
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n.to_owned()),
                _ => None,
            })
            .collect();
        rest.extend(todo.drain(..));
        todo = rest;
        chain.push(abs);
        cur = PathBuf::from("/");
    }
    chain
}

/// Resolve the symlinked `/etc` entries.
pub fn probe_etc_links(fs: &dyn Fs) -> EtcLinkFacts {
    let mut links = BTreeMap::new();
    for name in ETC_ALLOW_LIST.iter().chain(ETC_EXTRA_ENTRIES) {
        let path = Path::new("/etc").join(name);
        let Ok(md) = fs.metadata(&path) else { continue };
        if md.file_type != FileType::Symlink {
            continue;
        }
        let target = fs.read_link(&path).unwrap_or_default();
        let resolved = fs.canonicalize(&path).ok();
        let chain = resolve_chain(fs, &path);
        let via_etc_static = chain.iter().skip(1).any(|p| p.starts_with("/etc/static"));
        let is_dir = resolved
            .as_ref()
            .is_some_and(|r| fs.metadata_follow(r).is_ok_and(|m| m.is_dir()));
        links.insert(
            (*name).to_owned(),
            EtcLink {
                target,
                chain,
                resolved,
                is_dir,
                via_etc_static,
            },
        );
    }
    EtcLinkFacts { links }
}
