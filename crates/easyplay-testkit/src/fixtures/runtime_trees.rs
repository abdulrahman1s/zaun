//! Runtime store and external Proton trees for discovery and resolver tests (RT-003, RT-005).
//! Manifests follow the captured `UMU-Proton-10.0-4` and `steamrt3` files.

use std::path::{Path, PathBuf};

use easyplay_core::fsx::Fs;

use crate::MemFs;

/// `require_tool_appid` of sniper (`steamrt3`).
pub const SNIPER: &str = "1628350";
/// `require_tool_appid` of `steamrt4`.
pub const STEAMRT4: &str = "4183110";
/// `require_tool_appid` of soldier (unsupported).
pub const SOLDIER: &str = "1391110";

/// A Proton build directory. `appid = None` writes a manifest without a container runtime.
pub fn proton_build(
    fs: &MemFs,
    dir: impl AsRef<Path>,
    appid: Option<&str>,
    display_name: Option<&str>,
) {
    let dir = dir.as_ref();
    let req = appid
        .map(|a| format!("  \"require_tool_appid\" \"{a}\"\n"))
        .unwrap_or_default();
    fs.file(
        dir.join("toolmanifest.vdf"),
        format!(
            "\"manifest\"\n{{\n  \"version\" \"2\"\n  \"commandline\" \"/proton %verb%\"\n{req}  \"use_sessions\" \"1\"\n  \"compatmanager_layer_name\" \"proton\"\n}}\n"
        ),
    );
    fs.file(dir.join("proton"), "#!/usr/bin/env python3\n");
    fs.set_mode(&dir.join("proton"), 0o755).unwrap();
    fs.dir(dir.join("files/bin"));
    if let Some(name) = display_name {
        let internal = dir.file_name().unwrap().to_string_lossy();
        fs.file(
            dir.join("compatibilitytool.vdf"),
            format!(
                "\"compatibilitytools\"\n{{\n  \"compat_tools\"\n  {{\n    \"{internal}\" // Internal name\n    {{\n      \"install_path\" \".\"\n      \"display_name\" \"{name}\"\n      \"from_oslist\"  \"windows\"\n      \"to_oslist\"    \"linux\"\n    }}\n  }}\n}}\n"
            ),
        );
        fs.file(
            dir.join("protonfixes/umu-database.csv"),
            "TITLE,STORE,CODENAME,UMU_ID\n",
        );
    }
}

/// A Steam Linux Runtime variant directory as umu leaves it.
pub fn slr_variant(
    fs: &MemFs,
    dir: impl AsRef<Path>,
    platform: &str,
    version: &str,
    complete: bool,
) {
    let dir = dir.as_ref();
    fs.file(
        dir.join("toolmanifest.vdf"),
        "// Generated file, do not edit\n\"manifest\"\n{\n\t\"commandline\" \"/_v2-entry-point --verb=%verb% --\"\n\t\"version\" \"2\"\n\t\"compatmanager_layer_name\" \"container-runtime\"\n}\n",
    )
    .file(
        dir.join("VERSIONS.txt"),
        format!("#Name\tVersion\t\tRuntime\tRuntime_Version\tComment\ndepot\t{version}\t\t\t# Overall version number\n"),
    )
    .file(dir.join("_v2-entry-point"), "#!/bin/sh\n")
    .file(dir.join("run"), "#!/bin/sh\n")
    .file(dir.join("mtree.txt.gz"), "gz")
    .dir(dir.join("pressure-vessel/bin"))
    .dir(dir.join(format!("{platform}_platform_{version}/files")))
    .file(dir.join(format!("{platform}_platform_{version}/files/.ref")), "");
    if complete {
        fs.file(dir.join(".installed.ok"), "ok\n");
        fs.link(dir.join("umu"), "_v2-entry-point");
    }
}

/// Paths of [`standard_tree`].
pub struct RuntimeTree {
    /// `<data>`.
    pub data: PathBuf,
    /// `<cache>`.
    pub cache: PathBuf,
    /// The home directory.
    pub home: PathBuf,
}

/// A home with a store and external builds:
///
/// ```text
/// STORE/Steam/compatibilitytools.d/{GE-Proton10-9, GE-Proton10-25, UMU-Proton-10.0-4}
/// STORE/umu/steamrt3 (complete), STORE/umu/umu-shim
/// ~/.local/share/Steam/compatibilitytools.d/
///     UMU-Proton-9.0-1             valid, external
///     proton-cachyos-11.0          valid, steamrt4 (not installed in the store)
///     broken                       no proton script
///     host-runtime                 no require_tool_appid
///     soldier                      requires soldier
///     escape -> /opt/proton        symlink out of the folder
///     alias -> UMU-Proton-9.0-1    symlink inside the folder (same build, listed once)
/// ~/.steam/root -> ~/.local/share/Steam      (same builds again, not duplicated)
/// ~/.var/app/com.valvesoftware.Steam/…/GE-Proton9-1   (Flatpak data: never scanned)
/// ~/.local/share/umu/steamrt3, ~/.cache/umu/x.tar.xz.parts  (umu's own, informational)
/// ```
pub fn standard_tree(fs: &MemFs) -> RuntimeTree {
    let home = PathBuf::from("/home/sam");
    let data = home.join(".local/share/easyplay");
    let cache = home.join(".cache/easyplay");
    let compat = data.join("runtimes/Steam/compatibilitytools.d");
    proton_build(
        fs,
        compat.join("GE-Proton10-9"),
        Some(SNIPER),
        Some("GE-Proton10-9"),
    );
    proton_build(
        fs,
        compat.join("GE-Proton10-25"),
        Some(SNIPER),
        Some("GE-Proton10-25"),
    );
    proton_build(
        fs,
        compat.join("UMU-Proton-10.0-4"),
        Some(SNIPER),
        Some("UMU-Proton-10.0-4"),
    );
    slr_variant(
        fs,
        data.join("runtimes/umu/steamrt3"),
        "sniper",
        "3.0.20261110.268881",
        true,
    );
    fs.file(data.join("runtimes/umu/umu-shim"), "#!/bin/sh\n");
    fs.dir(data.join("runtimes/session-var"));

    let steam = home.join(".local/share/Steam/compatibilitytools.d");
    proton_build(fs, steam.join("UMU-Proton-9.0-1"), Some(SNIPER), None);
    proton_build(
        fs,
        steam.join("proton-cachyos-11.0"),
        Some(STEAMRT4),
        Some("proton-cachyos-11.0"),
    );
    proton_build(fs, steam.join("broken"), Some(SNIPER), None);
    fs.remove_file(&steam.join("broken/proton")).unwrap();
    proton_build(fs, steam.join("host-runtime"), None, None);
    proton_build(fs, steam.join("soldier"), Some(SOLDIER), None);
    proton_build(fs, "/opt/proton", Some(SNIPER), None);
    fs.link(steam.join("escape"), "/opt/proton");
    fs.link(steam.join("alias"), "UMU-Proton-9.0-1");
    fs.link(home.join(".steam/root"), home.join(".local/share/Steam"));
    proton_build(
        fs,
        home.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d/GE-Proton9-1"),
        Some(SNIPER),
        None,
    );
    fs.link(
        home.join(".config/heroic/tools/proton"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d"),
    );
    slr_variant(
        fs,
        home.join(".local/share/umu/steamrt3"),
        "sniper",
        "3.0.20261110.268881",
        true,
    );
    fs.file(
        home.join(".cache/umu/SteamLinuxRuntime_sniper.tar.xz.parts"),
        "partial",
    );
    RuntimeTree { data, cache, home }
}
