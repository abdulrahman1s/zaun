//! Whether the user can open a device node: the one question `Fs` cannot answer from
//! metadata alone (ACLs such as logind's `uaccess` grant access that mode bits do not show).

use std::path::Path;
use std::sync::Arc;

use crate::fsx::Fs;

/// Access checks on device nodes.
pub trait AccessCheck: Send + Sync {
    /// `access(path, R_OK)`.
    fn readable(&self, path: &Path) -> bool;
    /// `access(path, R_OK | W_OK)`.
    fn read_write(&self, path: &Path) -> bool;
}

/// `access(2)` through rustix (honours ACLs and supplementary groups).
#[derive(Clone, Copy, Debug, Default)]
pub struct RealAccess;

impl AccessCheck for RealAccess {
    fn readable(&self, path: &Path) -> bool {
        rustix::fs::access(path, rustix::fs::Access::READ_OK).is_ok()
    }
    fn read_write(&self, path: &Path) -> bool {
        rustix::fs::access(
            path,
            rustix::fs::Access::READ_OK | rustix::fs::Access::WRITE_OK,
        )
        .is_ok()
    }
}

/// Mode-bit evaluation against a user and its groups, through [`Fs`] (tests, and a fallback
/// that ignores ACLs).
pub struct ModeAccess {
    fs: Arc<dyn Fs>,
    uid: u32,
    groups: Vec<u32>,
}

impl ModeAccess {
    /// Evaluate for `uid` with primary and supplementary `groups`.
    pub fn new(fs: Arc<dyn Fs>, uid: u32, groups: Vec<u32>) -> Self {
        Self { fs, uid, groups }
    }

    fn allows(&self, path: &Path, want: u32) -> bool {
        let Ok(md) = self.fs.metadata_follow(path) else {
            return false;
        };
        if self.uid == 0 {
            return true;
        }
        let bits = if md.uid == self.uid {
            md.mode >> 6
        } else if self.groups.contains(&md.gid) {
            md.mode >> 3
        } else {
            md.mode
        };
        bits & want == want
    }
}

impl AccessCheck for ModeAccess {
    fn readable(&self, path: &Path) -> bool {
        self.allows(path, 0o4)
    }
    fn read_write(&self, path: &Path) -> bool {
        self.allows(path, 0o6)
    }
}
