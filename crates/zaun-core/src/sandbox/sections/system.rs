//! §4.2 System directories and §4.14 umu closure plus the init shim (the `Umu` section).

use std::path::{Path, PathBuf};

use super::mk;
use crate::host::{HostEntry, HostFacts, LayoutKind};
use crate::paths::normalize;
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{INIT_PATH, PlanBuilder};

/// Top-level directories handled by the merged-`/usr` rule, in emission order.
pub const TOP_LEVEL: [&str; 6] = ["bin", "sbin", "lib", "lib32", "lib64", "libx32"];

/// §4.2.
pub fn emit(ctx: &SandboxContext<'_>, host: &HostFacts, b: &mut PlanBuilder) {
    let layout = &host.layout;
    b.mount(mk::ro_same("/usr"), "system libraries and programs", None);
    for name in TOP_LEVEL {
        let path = Path::new("/").join(name);
        match layout.top_level.get(&path) {
            Some(HostEntry::Symlink { target, .. }) => {
                b.mount(mk::symlink(target.clone(), &path), "merged /usr link", None);
            }
            Some(HostEntry::Dir) => b.mount(mk::ro_same(&path), "system directory", None),
            Some(HostEntry::File | HostEntry::Other) | None => {}
        }
    }
    b.mount(mk::ro_try("/opt", "/opt"), "vendor drivers and tools", None);
    match layout.kind {
        LayoutKind::Nix => {
            b.mount(mk::ro_same("/nix"), "Nix store", None);
            b.mount(
                mk::ro_same("/run/current-system"),
                "NixOS system profile",
                None,
            );
            for p in ["/run/opengl-driver", "/run/opengl-driver-32", "/etc/static"] {
                b.mount(
                    mk::ro_try(p, p),
                    "NixOS graphics drivers and configuration",
                    None,
                );
            }
            let profile = Path::new("/etc/profiles/per-user").join(ctx.user_name);
            b.mount(mk::ro_try(&profile, &profile), "NixOS user profile", None);
        }
        LayoutKind::Guix => {
            b.mount(mk::ro_same("/gnu"), "Guix store", None);
            b.mount(
                mk::ro_try("/run/current-system", "/run/current-system"),
                "Guix system profile",
                None,
            );
        }
        LayoutKind::Fhs => {}
    }
    let mut extra: Vec<&PathBuf> = layout.extra_lib_roots.iter().collect();
    extra.sort();
    extra.dedup();
    for root in extra {
        b.mount(
            mk::ro_same(root),
            "extra library directory from ld.so.conf",
            None,
        );
    }
    b.mount(mk::dir("/var"), "empty /var", None);
    b.mount(
        mk::symlink("../run", "/var/run"),
        "/var/run compatibility link",
        None,
    );
    b.mount(
        mk::tmpfs("/var/tmp", Some(0o1777), None),
        "private /var/tmp",
        None,
    );
    b.mount(
        mk::ro_try("/var/cache/fontconfig", "/var/cache/fontconfig"),
        "font cache",
        None,
    );
}

/// §4.14: umu closure entries not covered by a system bind, then the init shim.
pub fn emit_umu(ctx: &SandboxContext<'_>, host: &HostFacts, b: &mut PlanBuilder) {
    let roots = host.layout.system_roots();
    let mut closure: Vec<PathBuf> = ctx.umu.closure.iter().map(|p| normalize(p)).collect();
    closure.sort();
    closure.dedup();
    for entry in closure {
        if roots.iter().any(|r| entry.starts_with(r)) {
            continue;
        }
        b.mount(
            mk::ro_same(&entry),
            "umu-run and its Python environment",
            None,
        );
    }
    b.mount(mk::ro(ctx.init_binary, INIT_PATH), "Zaun init shim", None);
}
