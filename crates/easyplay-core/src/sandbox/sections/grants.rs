//! §4.13 Folder grants, `ExtraReadOnlyPath` exceptions and removable media.
//!
//! Sorted by destination depth, then lexicographically, so a parent is mounted before a child
//! grant with different access.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use super::mk;
use crate::model::{FolderAccess, PolicyException, RemovableAccess, SandboxPolicy};
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{MountMode, PlanBuilder, PolicyField};

/// Sort key: (component count, bytes).
pub fn order_key(p: &Path) -> (usize, Vec<u8>) {
    (p.components().count(), p.as_os_str().as_bytes().to_vec())
}

/// §4.13.
pub fn emit(policy: &SandboxPolicy, ctx: &SandboxContext<'_>, b: &mut PlanBuilder) {
    if !ctx.untrusted() {
        return;
    }
    let mut items: Vec<(PathBuf, MountMode, PolicyField, &'static str)> = Vec::new();
    for g in &policy.folders {
        let mode = match g.access {
            FolderAccess::ReadOnly => MountMode::Ro,
            FolderAccess::ReadWrite => MountMode::Rw,
        };
        items.push((
            g.canonical.clone(),
            mode,
            PolicyField::Folder(g.canonical.clone()),
            "folder you shared",
        ));
    }
    if ctx.developer_mode {
        for e in &policy.exceptions {
            if let PolicyException::ExtraReadOnlyPath { canonical, .. } = e {
                items.push((
                    canonical.clone(),
                    MountMode::Ro,
                    PolicyField::ExtraReadOnlyPath(canonical.clone()),
                    "read-only exception",
                ));
            }
        }
    }
    let removable_mode = match policy.removable {
        RemovableAccess::None => None,
        RemovableAccess::ReadOnly => Some(MountMode::Ro),
        RemovableAccess::ReadWrite => Some(MountMode::Rw),
    };
    let mut entries: Vec<(PathBuf, MountMode, PolicyField, &'static str, bool)> = items
        .into_iter()
        .map(|(p, m, f, r)| (p, m, f, r, false))
        .collect();
    if let Some(mode) = removable_mode {
        for p in removable_dirs(ctx.user_name) {
            if ctx.fs.exists(&p) {
                entries.push((p, mode, PolicyField::Removable, "removable drives", true));
            }
        }
    }
    // One ordering for grants and removable media: parents always precede children.
    entries.sort_by_key(|e| order_key(&e.0));
    for (path, mode, field, reason, try_) in entries {
        b.mount(mk::bind(&path, &path, mode, try_), reason, Some(field));
    }
}

/// `/run/media/<user>` and `/media/<user>`.
pub fn removable_dirs(user: &str) -> [PathBuf; 2] {
    [
        Path::new("/run/media").join(user),
        Path::new("/media").join(user),
    ]
}
