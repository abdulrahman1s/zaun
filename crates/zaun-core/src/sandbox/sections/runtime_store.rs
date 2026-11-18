//! §4.9 Runtime store and §4.10 Proton.
//!
//! Three layouts: trusted network sessions see the store, all Proton builds and umu's cache
//! read-write (the cache keeps interrupted downloads resumable, TECHNICAL_RESEARCH F-225);
//! Isolated mode mounts the store read-only with a synthesized SLR top level; Shared mode
//! mounts the store read-write and requires an audit afterwards. Proton is always read-only
//! outside trusted network sessions.

use std::path::{Path, PathBuf};

use super::{Effective, SandboxDirs, mk};
use crate::fsx::FileType;
use crate::model::{ResolvedRuntime, RuntimeIsolationMode, RuntimeOrigin};
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{EnvOrigin, FileDataOp, HostAction, PV_VAR_PATH, PlanBuilder};
use crate::sandbox::validate::{IssueCode, PolicyIssue};

/// Top-level SLR entries that are never bound in Isolated mode.
pub const SKIPPED_ENTRIES: [&str; 2] = ["var", ".installed.ok"];

/// Content seeded into the synthesized `.installed.ok` marker.
pub const INSTALLED_OK: &[u8] = b"ok\n";

/// The platform directory's name relative to the SLR directory.
pub fn platform_name(rt: &ResolvedRuntime) -> PathBuf {
    if rt.platform_dir.is_absolute() {
        rt.platform_dir
            .strip_prefix(&rt.slr_dir)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| {
                rt.platform_dir
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_default()
            })
    } else {
        rt.platform_dir.clone()
    }
}

fn runtime_error(b: &mut PlanBuilder, path: &Path, what: &str) {
    b.issue(PolicyIssue::error(
        IssueCode::RuntimeLayout,
        Some(path.to_path_buf()),
        format!("sec.runtime.layout|The compatibility layer is incomplete: {what}"),
    ));
}

/// §4.9–4.10.
pub fn emit(ctx: &SandboxContext<'_>, eff: &Effective, b: &mut PlanBuilder) {
    let dirs = SandboxDirs::new(ctx.home, ctx.uid);
    let store_umu = &ctx.paths.umu_store_dir;
    if eff.trusted_network {
        for (src, dst, reason) in [
            (store_umu, &dirs.umu, "runtime store (installing)"),
            (
                &ctx.paths.compat_tools_dir,
                &dirs.compat,
                "compatibility layers (installing)",
            ),
            (
                &ctx.paths.umu_cache_dir,
                &dirs.cache,
                "download cache (resumable)",
            ),
        ] {
            b.action(HostAction::CreateDir {
                path: src.clone(),
                mode: 0o700,
            });
            b.mount(mk::rw(src, dst), reason, None);
        }
        let protonpath = match (ctx.proton_codename, ctx.runtime) {
            (Some(code), _) => Some(code.to_owned()),
            (None, Some(rt)) => {
                if rt.origin == RuntimeOrigin::External {
                    b.mount(
                        mk::ro_same(&rt.proton_path_host),
                        "selected compatibility layer",
                        None,
                    );
                }
                Some(rt.proton_path_sandbox.display().to_string())
            }
            (None, None) => None,
        };
        if let Some(pp) = protonpath {
            b.env("PROTONPATH", pp, EnvOrigin::Fixed);
        }
        return;
    }
    let Some(rt) = ctx.runtime else {
        b.issue(PolicyIssue::error(
            IssueCode::ContextInvalid,
            None,
            "sec.context.no_runtime|No compatibility layer was resolved",
        ));
        return;
    };
    let rt_dst = dirs.umu.join(rt.slr_variant.as_str());
    match eff.isolation.unwrap_or(rt.isolation) {
        RuntimeIsolationMode::Isolated => isolated(ctx, rt, &dirs, &rt_dst, b),
        RuntimeIsolationMode::Shared => {
            b.mount(
                mk::rw(store_umu, &dirs.umu),
                "shared runtime store (audited)",
                None,
            );
            b.note(
                "note.runtime.shared",
                "The compatibility layer is shared and checked after the session",
            );
        }
    }
    // §4.10: only the selected build, read-only.
    let (src, dst) = match rt.origin {
        RuntimeOrigin::Store => (rt.proton_path_host.clone(), rt.proton_path_sandbox.clone()),
        RuntimeOrigin::External => (rt.proton_path_host.clone(), rt.proton_path_host.clone()),
    };
    b.mount(mk::ro(&src, &dst), "selected compatibility layer", None);
    b.env("PROTONPATH", dst.display().to_string(), EnvOrigin::Fixed);
}

fn isolated(
    ctx: &SandboxContext<'_>,
    rt: &ResolvedRuntime,
    dirs: &SandboxDirs,
    rt_dst: &Path,
    b: &mut PlanBuilder,
) {
    b.mount(
        mk::ro(&ctx.paths.umu_store_dir, &dirs.umu),
        "runtime store, read-only",
        None,
    );
    b.mount(
        mk::tmpfs(rt_dst, None, None),
        "synthesized runtime top level",
        None,
    );
    let mut entries = match ctx.fs.read_dir(&rt.slr_dir) {
        Ok(e) => e,
        Err(_) => {
            runtime_error(b, &rt.slr_dir, "the runtime directory cannot be listed");
            return;
        }
    };
    entries.sort_by(|a, b| {
        use std::os::unix::ffi::OsStrExt;
        a.name.as_bytes().cmp(b.name.as_bytes())
    });
    for e in entries {
        if SKIPPED_ENTRIES.iter().any(|s| e.name == *s) {
            continue;
        }
        let dst = rt_dst.join(&e.name);
        if e.file_type == FileType::Symlink {
            match ctx.fs.read_link(&e.path) {
                Ok(target) => b.mount(mk::symlink(target, &dst), "runtime link", None),
                Err(_) => runtime_error(b, &e.path, "a link cannot be read"),
            }
        } else {
            b.mount(mk::ro(&e.path, &dst), "runtime file, read-only", None);
        }
    }
    b.file_data(
        rt_dst.join(".installed.ok"),
        INSTALLED_OK.to_vec(),
        0o644,
        FileDataOp::File,
        "runtime marker (replaced by umu when offline)",
        None,
    );
    let platform = platform_name(rt);
    let lock_rel = platform.join("files/.ref");
    let lock_src = rt.slr_dir.join(&lock_rel);
    if !ctx.fs.exists(&lock_src) {
        runtime_error(b, &lock_src, "the runtime lock file is missing");
    }
    b.mount(
        mk::rw(&lock_src, rt_dst.join(&lock_rel)),
        "runtime lock file (shared on purpose)",
        None,
    );
    b.action(HostAction::CreateDir {
        path: ctx.session.var_dir.clone(),
        mode: 0o700,
    });
    b.mount(
        mk::rw(&ctx.session.var_dir, PV_VAR_PATH),
        "per-session runtime copy",
        None,
    );
    b.env(
        "PRESSURE_VESSEL_VARIABLE_DIR",
        PV_VAR_PATH,
        EnvOrigin::Fixed,
    );
}
