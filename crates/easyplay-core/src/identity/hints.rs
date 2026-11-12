//! Setup hints: `installer_like`, `online_capable`, the default role, and what the game folder
//! reveals (anti-cheat, Steam DRM, `steam_appid.txt`). GAME_IDENTITY §8 and §10, LOGIC_SPECS §2.
//!
//! Hints only pre-select choices and explain things in the setup dialog. They never loosen a
//! policy: no hint grants network access, a permission or a different sandbox.

use std::path::Path;

use crate::clock::Clock;
use crate::fsx::{FileType, Fs, WalkControl, WalkLimits, safe_walk};
use crate::model::{ExeRole, PeInfo};

use super::pe::note;

/// Most directory entries looked at when listing an executable's siblings.
pub const MAX_SIBLINGS: usize = 512;
/// Largest `steam_appid.txt` read (ADR-0018 §3.2).
pub const STEAM_APPID_MAX_BYTES: u64 = 64;
/// Loose executables larger than this whose version strings name an installer framework are
/// installer-like.
pub const INSTALLER_FRAMEWORK_MIN_BYTES: u64 = 20 * 1024 * 1024;

/// An anti-cheat system detected by its files next to the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AntiCheat {
    /// Easy Anti-Cheat (`EasyAntiCheat*`).
    EasyAntiCheat,
    /// BattlEye (`BEClient*`, `BEService*`, a `BattlEye` folder).
    BattlEye,
}

/// What the executable and its folder reveal; informational only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExeHints {
    /// Anti-cheat systems found (sorted, unique). Their kernel or online checks may refuse
    /// to run under Proton or in a sandbox.
    pub anti_cheat: Vec<AntiCheat>,
    /// A `steam_api*.dll` is present: the game talks to Steam.
    pub steam_api: bool,
    /// The executable has a `.bind` section (SteamStub DRM: needs a running Steam client).
    pub steam_drm: bool,
    /// Epic Online Services or GOG Galaxy SDK present.
    pub store_sdk: bool,
}

/// Inputs of [`setup_hints`].
#[derive(Clone, Copy, Debug)]
pub struct HintInput<'a> {
    /// File name of the executable.
    pub file_name: &'a str,
    /// File size.
    pub size: u64,
    /// PE information (possibly [`PeInfo::unknown`]).
    pub pe: &'a PeInfo,
    /// Imported DLL names, lower-case.
    pub imports: &'a [String],
    /// Names of the entries next to the executable (and in the game root, when different),
    /// from [`list_names`].
    pub siblings: &'a [String],
    /// The executable sits in a shared location (no game folder).
    pub loose: bool,
    /// The executable is inside the root of a game that is already in the library.
    pub inside_known_root: bool,
}

/// Hints for the setup dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetupHints {
    /// Probably an installer (GAME_IDENTITY §8).
    pub installer_like: bool,
    /// Probably uses the network (LOGIC_SPECS §2 step 6). Shown as a suggestion only.
    pub online_capable: bool,
    /// Role to pre-select, when not the default (`Primary` for a new game).
    pub default_role: Option<ExeRole>,
    /// Folder findings.
    pub exe: ExeHints,
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// `installer_like` of GAME_IDENTITY §8.
pub fn installer_like(input: &HintInput<'_>) -> bool {
    let name = lower(input.file_name);
    let by_name = name.ends_with(".msi")
        || (name.ends_with(".exe")
            && (name.starts_with("setup")
                || name.starts_with("install")
                || name.ends_with("_setup.exe")
                || name.starts_with("gog_")));
    let text = |s: &Option<String>| s.as_deref().map(lower).unwrap_or_default();
    let product = text(&input.pe.product_name);
    let desc = text(&input.pe.file_description);
    let by_strings = [&product, &desc]
        .iter()
        .any(|s| s.contains("setup") || s.contains("installer"));
    let company = text(&input.pe.company);
    let framework = [&product, &desc, &company].iter().any(|s| {
        ["inno setup", "nullsoft", "nsis", "installshield", "wix"]
            .iter()
            .any(|f| s.contains(f))
    });
    by_name
        || by_strings
        || (input.loose && input.size > INSTALLER_FRAMEWORK_MIN_BYTES && framework)
}

/// `online_capable` of LOGIC_SPECS §2 step 6.
pub fn online_capable(imports: &[String], siblings: &[String]) -> bool {
    let net_import = imports.iter().any(|i| {
        matches!(
            i.as_str(),
            "ws2_32.dll" | "wininet.dll" | "winhttp.dll" | "wsock32.dll"
        )
    });
    net_import || siblings.iter().any(|s| is_online_sdk(&lower(s)))
}

fn is_dll_named(name: &str, prefix: &str) -> bool {
    name.starts_with(prefix) && name.ends_with(".dll")
}

fn is_online_sdk(name: &str) -> bool {
    is_dll_named(name, "steam_api") || is_dll_named(name, "eossdk") || is_dll_named(name, "galaxy")
}

/// Findings from the executable's PE notes and the names next to it.
pub fn exe_hints(pe: &PeInfo, siblings: &[String]) -> ExeHints {
    let mut h = ExeHints {
        steam_drm: pe.notes.iter().any(|n| n == note::STEAM_DRM),
        ..ExeHints::default()
    };
    for s in siblings {
        let n = lower(s);
        if n.starts_with("easyanticheat") {
            h.anti_cheat.push(AntiCheat::EasyAntiCheat);
        }
        if n.starts_with("beclient") || n.starts_with("beservice") || n == "battleye" {
            h.anti_cheat.push(AntiCheat::BattlEye);
        }
        if is_dll_named(&n, "steam_api") {
            h.steam_api = true;
        }
        if is_dll_named(&n, "eossdk") || is_dll_named(&n, "galaxy") {
            h.store_sdk = true;
        }
    }
    h.anti_cheat.sort();
    h.anti_cheat.dedup();
    h
}

/// The role setup pre-selects (GAME_IDENTITY §8): `Tool` for `config*`, `settings*` and
/// `setup*` files inside a known game root; `Installer` for installer-like files elsewhere.
pub fn default_role(input: &HintInput<'_>, installer_like: bool) -> Option<ExeRole> {
    let name = lower(input.file_name);
    if input.inside_known_root
        && ["config", "settings", "setup"]
            .iter()
            .any(|p| name.starts_with(p))
    {
        return Some(ExeRole::Tool);
    }
    installer_like.then_some(ExeRole::Installer)
}

/// All hints for one executable.
pub fn setup_hints(input: &HintInput<'_>) -> SetupHints {
    let installer = installer_like(input);
    SetupHints {
        installer_like: installer,
        online_capable: online_capable(input.imports, input.siblings),
        default_role: default_role(input, installer),
        exe: exe_hints(input.pe, input.siblings),
    }
}

/// Names of at most [`MAX_SIBLINGS`] entries directly in `dir` (no recursion, no symlink
/// following). Errors yield an empty list: hints are best effort.
pub fn list_names(fs: &dyn Fs, clock: &dyn Clock, dir: &Path) -> Vec<String> {
    let limits = WalkLimits {
        max_entries: MAX_SIBLINGS,
        max_depth: 1,
        ..WalkLimits::with_budget(clock, std::time::Duration::from_millis(500))
    };
    let mut out = Vec::new();
    let _ = safe_walk(fs, dir, limits, clock, |e| {
        if let Some(n) = e.path.file_name() {
            out.push(n.to_string_lossy().into_owned());
        }
        WalkControl::SkipDir
    });
    out
}

/// The Steam app ID in `<dir>/steam_appid.txt` (ADR-0018 §3.2 step 2): a regular file (a
/// symlink is ignored) of at most [`STEAM_APPID_MAX_BYTES`] bytes containing only ASCII
/// digits, optionally surrounded by whitespace. Anything else, or a missing file, is `None`.
pub fn read_steam_appid(fs: &dyn Fs, dir: &Path) -> Option<u32> {
    let path = dir.join("steam_appid.txt");
    let md = fs.metadata(&path).ok()?;
    if md.file_type != FileType::File || md.size > STEAM_APPID_MAX_BYTES {
        return None;
    }
    let bytes = fs.read(&path, STEAM_APPID_MAX_BYTES).ok()?;
    parse_steam_appid(&bytes)
}

/// The parsing rule of [`read_steam_appid`].
pub fn parse_steam_appid(bytes: &[u8]) -> Option<u32> {
    if bytes.len() as u64 > STEAM_APPID_MAX_BYTES {
        return None;
    }
    let s = std::str::from_utf8(bytes).ok()?;
    let s = s.strip_prefix('\u{FEFF}').unwrap_or(s).trim_ascii();
    if s.is_empty() || s.len() > 10 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u32>().ok().filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(name: &'a str, pe: &'a PeInfo, siblings: &'a [String]) -> HintInput<'a> {
        HintInput {
            file_name: name,
            size: 1000,
            pe,
            imports: &[],
            siblings,
            loose: false,
            inside_known_root: false,
        }
    }

    #[test]
    fn installer_names() {
        let pe = PeInfo::default();
        for (name, expected) in [
            ("setup.exe", true),
            ("Setup_Game_1.2.exe", true),
            ("installer.exe", true),
            ("Install-Game.exe", true),
            ("game_setup.exe", true),
            ("gog_galaxy_x.exe", true),
            ("game.msi", true),
            ("Game.exe", false),
            ("setup.dll", false),
            ("UnityCrashHandler64.exe", false),
        ] {
            assert_eq!(installer_like(&input(name, &pe, &[])), expected, "{name}");
        }
    }

    #[test]
    fn installer_strings_and_frameworks() {
        let pe = PeInfo {
            file_description: Some("Game Setup".into()),
            ..PeInfo::default()
        };
        assert!(installer_like(&input("x.exe", &pe, &[])));
        let inno = PeInfo {
            company: Some("Inno Setup developers".into()),
            ..PeInfo::default()
        };
        let mut i = input("x.exe", &inno, &[]);
        assert!(!installer_like(&i));
        i.loose = true;
        i.size = 30 * 1024 * 1024;
        assert!(installer_like(&i));
    }

    #[test]
    fn roles() {
        let pe = PeInfo::default();
        let mut i = input("Config.exe", &pe, &[]);
        assert_eq!(default_role(&i, false), None);
        i.inside_known_root = true;
        assert_eq!(default_role(&i, false), Some(ExeRole::Tool));
        let mut s = input("setup.exe", &pe, &[]);
        assert_eq!(default_role(&s, true), Some(ExeRole::Installer));
        s.inside_known_root = true;
        assert_eq!(default_role(&s, true), Some(ExeRole::Tool));
    }

    #[test]
    fn online_and_folder() {
        let sib: Vec<String> = [
            "steam_api64.dll",
            "EasyAntiCheat",
            "BEClient_x64.dll",
            "x.pak",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        assert!(online_capable(&[], &sib));
        assert!(online_capable(&["ws2_32.dll".into()], &[]));
        assert!(!online_capable(
            &["kernel32.dll".into()],
            &["data.pak".into()]
        ));
        let pe = PeInfo {
            notes: vec![note::STEAM_DRM.into()],
            ..PeInfo::default()
        };
        let h = exe_hints(&pe, &sib);
        assert_eq!(
            h.anti_cheat,
            vec![AntiCheat::EasyAntiCheat, AntiCheat::BattlEye]
        );
        assert!(h.steam_api && h.steam_drm && !h.store_sdk);
    }

    #[test]
    fn steam_appid_parsing() {
        assert_eq!(parse_steam_appid(b"367520"), Some(367520));
        assert_eq!(parse_steam_appid(b" 367520\r\n"), Some(367520));
        assert_eq!(parse_steam_appid("\u{FEFF}480".as_bytes()), Some(480));
        assert_eq!(parse_steam_appid(b""), None);
        assert_eq!(parse_steam_appid(b"0"), None);
        assert_eq!(parse_steam_appid(b"12a"), None);
        assert_eq!(parse_steam_appid(b"-5"), None);
        assert_eq!(parse_steam_appid(b"99999999999"), None);
        assert_eq!(parse_steam_appid(&[b'1'; 65]), None);
    }
}
