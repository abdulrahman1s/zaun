//! A fake Steam installation (ADR-0031 tests), written into a [`MemFs`] or onto disk. The file
//! contents are scrubbed copies of a real installation's (`easyplay-core/tests/data/steam`);
//! no test reads the real `~/.local/share/Steam`.
//!
//! ```text
//! <home>/.local/share/Steam                    the root (native)
//!   steamapps/libraryfolders.vdf               the root and <library>
//!   steamapps/appmanifest_228980.acf           Steamworks Common Redistributables (not a game)
//!   steamapps/appmanifest_1493710.acf          Proton Experimental (not a game; toolmanifest)
//!   steamapps/appmanifest_440.acf              Team Fortress 2: update paused, Linux version
//!   config/config.vdf                          CompatToolMapping: 1174180 → GE-Proton10-25
//!   config/loginusers.vdf                      two accounts; the first is the most recent
//!   userdata/39734273/config/localconfig.vdf   play times
//!   appcache/librarycache/440/library_600x900.jpg                     cover, per-app layout
//!   appcache/librarycache/1174180/<sha1>/library_capsule.jpg          cover, hashed layout
//!   appcache/librarycache/12210_library_600x900.jpg                   cover, flat layout
//!   compatibilitytools.d/GE-Proton10-25/       the Proton build of config_info
//! <home>/.steam/steam, <home>/.steam/root      symbolic links to the root
//! <library>/steamapps/appmanifest_1174180.acf  Red Dead Redemption 2: update waiting, Proton
//! <library>/steamapps/appmanifest_12210.acf    GTA IV: up to date
//! <library>/steamapps/appmanifest_4183110.acf  Steam Linux Runtime 4.0 (not a game)
//! <library>/steamapps/compatdata/1174180/      pfx/, config_info, version
//! ```

use std::path::{Path, PathBuf};

use crate::MemFs;

const DATA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../easyplay-core/tests/data/steam"
);

fn data(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DATA}/{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// The games the fixture lists, by app ID.
pub const GAMES: [u32; 3] = [440, 12210, 1_174_180];
/// The apps it leaves out, by app ID.
pub const NOT_GAMES: [u32; 3] = [228_980, 1_493_710, 4_183_110];
/// The account folder of the most recent user.
pub const ACCOUNT: &str = "39734273";

/// Where the fixture put things.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamFixture {
    /// The root.
    pub root: PathBuf,
    /// The second library folder.
    pub library: PathBuf,
    /// The Proton build `config_info` names.
    pub proton: PathBuf,
    /// RDR2's compatibility data.
    pub compat_data: PathBuf,
}

/// Something files can be written to.
trait Sink {
    fn dir(&self, p: &Path);
    fn file(&self, p: &Path, data: &[u8]);
    fn link(&self, link: &Path, target: &Path);
}

impl Sink for MemFs {
    fn dir(&self, p: &Path) {
        MemFs::dir(self, p);
    }
    fn file(&self, p: &Path, data: &[u8]) {
        MemFs::file(self, p, data);
    }
    fn link(&self, link: &Path, target: &Path) {
        MemFs::link(self, link, target);
    }
}

struct Disk;

impl Sink for Disk {
    fn dir(&self, p: &Path) {
        std::fs::create_dir_all(p).unwrap();
    }
    fn file(&self, p: &Path, data: &[u8]) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, data).unwrap();
    }
    fn link(&self, link: &Path, target: &Path) {
        if let Some(d) = link.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::os::unix::fs::symlink(target, link).unwrap();
    }
}

fn library_folders(root: &Path, library: &Path) -> String {
    format!(
        "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"label\"\t\t\"\"\n\t\t\"apps\"\n\t\t{{\n\t\t\t\"228980\"\t\t\"1\"\n\t\t\t\"1493710\"\t\t\"1\"\n\t\t\t\"440\"\t\t\"1\"\n\t\t}}\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"label\"\t\t\"Games\"\n\t}}\n}}\n",
        root.display(),
        library.display()
    )
}

fn proton_build(s: &dyn Sink, dir: &Path) {
    s.file(
        &dir.join("toolmanifest.vdf"),
        b"\"manifest\"\n{\n  \"version\" \"2\"\n  \"commandline\" \"/proton %verb%\"\n  \"require_tool_appid\" \"1628350\"\n  \"use_sessions\" \"1\"\n  \"compatmanager_layer_name\" \"proton\"\n}\n",
    );
    s.file(&dir.join("proton"), b"#!/usr/bin/env python3\n");
    s.dir(&dir.join("files/bin"));
}

fn write(s: &dyn Sink, home: &Path, library: &Path) -> SteamFixture {
    let root = home.join(".local/share/Steam");
    let apps = root.join("steamapps");
    let lib_apps = library.join("steamapps");
    let proton = root.join("compatibilitytools.d/GE-Proton10-25");
    let compat = lib_apps.join("compatdata/1174180");

    s.file(
        &apps.join("libraryfolders.vdf"),
        library_folders(&root, library).as_bytes(),
    );
    for id in ["228980", "1493710", "440"] {
        let name = format!("appmanifest_{id}.acf");
        s.file(&apps.join(&name), &data(&name));
    }
    s.dir(&apps.join("common/Steamworks Shared"));
    proton_build(s, &apps.join("common/Proton - Experimental"));
    s.dir(&apps.join("common/Team Fortress 2"));
    s.file(&root.join("config/config.vdf"), &data("config.vdf"));
    s.file(&root.join("config/loginusers.vdf"), &data("loginusers.vdf"));
    s.file(
        &root.join(format!("userdata/{ACCOUNT}/config/localconfig.vdf")),
        &data("localconfig.vdf"),
    );
    let cache = root.join("appcache/librarycache");
    s.file(
        &cache.join("440/library_600x900.jpg"),
        b"\xFF\xD8\xFFcover-440",
    );
    s.file(&cache.join("440/library_hero.jpg"), b"\xFF\xD8\xFFhero-440");
    s.file(
        &cache.join("1174180/0123456789abcdef0123456789abcdef01234567/library_capsule.jpg"),
        b"\xFF\xD8\xFFcover-1174180",
    );
    s.file(
        &cache.join("12210_library_600x900.jpg"),
        b"\xFF\xD8\xFFcover-12210",
    );
    proton_build(s, &proton);
    s.link(&home.join(".steam/steam"), &root);
    s.link(&home.join(".steam/root"), &root);

    for id in ["1174180", "12210", "4183110"] {
        let name = format!("appmanifest_{id}.acf");
        s.file(&lib_apps.join(&name), &data(&name));
    }
    s.dir(&lib_apps.join("common/Red Dead Redemption 2"));
    s.dir(&lib_apps.join("common/Grand Theft Auto IV"));
    s.file(
        &lib_apps.join("common/SteamLinuxRuntime_4/toolmanifest.vdf"),
        b"\"manifest\" { \"compatmanager_layer_name\" \"container-runtime\" }",
    );
    s.dir(&compat.join("pfx/drive_c/windows/system32"));
    s.file(
        &compat.join("config_info"),
        format!(
            "GE-Proton10-25\n{0}/files/share/fonts/\n{0}/files/lib/\n{1}\n1788403030.0\n",
            proton.display(),
            root.display()
        )
        .as_bytes(),
    );
    s.file(&compat.join("version"), b"GE-Proton10-25\n");
    SteamFixture {
        root,
        library: library.to_path_buf(),
        proton,
        compat_data: compat,
    }
}

/// Write the fake installation into `fs` under `home`, with the second library at
/// `/mnt/games/SteamLibrary`.
pub fn steam_root(fs: &MemFs, home: &Path) -> SteamFixture {
    write(fs, home, Path::new("/mnt/games/SteamLibrary"))
}

/// Write the fake installation onto disk under `home`, with the second library at
/// `<home>/mnt/SteamLibrary` (command-line tests).
pub fn steam_root_on_disk(home: &Path) -> SteamFixture {
    write(&Disk, home, &home.join("mnt/SteamLibrary"))
}
