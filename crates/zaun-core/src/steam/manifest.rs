//! `steamapps/appmanifest_<appid>.acf`: what Steam installed (ADR-0031 §2, §7).
//!
//! The update state comes from `StateFlags` only (Steam's `EAppState` bits). The byte counts
//! and `TargetBuildID` are not used to decide it: on a real machine several manifests with
//! flags `4` (installed, nothing waiting) carry unequal byte counts from the last update.

use std::path::PathBuf;

use super::keyvalues::{KvLimits, KvObject, parse};
use crate::clock::Timestamp;
use crate::model::{SteamDownload, SteamUpdate};

/// `EAppState` bits (Steamworks `steam_api` headers, `isteamapps.h`).
pub mod flags {
    /// `k_EAppStateUninstalled`.
    pub const UNINSTALLED: u64 = 1;
    /// `k_EAppStateUpdateRequired`.
    pub const UPDATE_REQUIRED: u64 = 2;
    /// `k_EAppStateFullyInstalled`.
    pub const FULLY_INSTALLED: u64 = 4;
    /// `k_EAppStateFilesMissing`.
    pub const FILES_MISSING: u64 = 32;
    /// `k_EAppStateAppRunning`.
    pub const APP_RUNNING: u64 = 64;
    /// `k_EAppStateFilesCorrupt`.
    pub const FILES_CORRUPT: u64 = 128;
    /// `k_EAppStateUpdateRunning`.
    pub const UPDATE_RUNNING: u64 = 256;
    /// `k_EAppStateUpdatePaused`.
    pub const UPDATE_PAUSED: u64 = 512;
    /// `k_EAppStateUpdateStarted`.
    pub const UPDATE_STARTED: u64 = 1024;
    /// `k_EAppStateReconfiguring`.
    pub const RECONFIGURING: u64 = 1 << 16;
    /// `k_EAppStateValidating`.
    pub const VALIDATING: u64 = 1 << 17;
    /// `k_EAppStateAddingFiles`.
    pub const ADDING_FILES: u64 = 1 << 18;
    /// `k_EAppStatePreallocating`.
    pub const PREALLOCATING: u64 = 1 << 19;
    /// `k_EAppStateDownloading`.
    pub const DOWNLOADING: u64 = 1 << 20;
    /// `k_EAppStateStaging`.
    pub const STAGING: u64 = 1 << 21;
    /// `k_EAppStateCommitting`.
    pub const COMMITTING: u64 = 1 << 22;
}

/// Largest manifest read.
pub const MANIFEST_MAX: u64 = 1024 * 1024;

/// One parsed manifest. Fields Steam did not write are `None` / 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppManifest {
    /// `appid` (never 0).
    pub app_id: u32,
    /// `name` (control characters removed, at most 80 characters).
    pub name: String,
    /// `installdir`: a single folder name under `steamapps/common`.
    pub install_dir: String,
    /// `StateFlags`.
    pub state_flags: u64,
    /// `SizeOnDisk`.
    pub size_on_disk: u64,
    /// `buildid`.
    pub build_id: Option<u64>,
    /// `LastUpdated` (Unix seconds).
    pub last_updated: Option<Timestamp>,
    /// `LastPlayed` (Unix seconds; 0 = never).
    pub last_played: Option<Timestamp>,
    /// `BytesToDownload`.
    pub bytes_to_download: u64,
    /// `BytesDownloaded`.
    pub bytes_downloaded: u64,
}

impl AppManifest {
    /// Whether Steam considers the game installed (`FullyInstalled`). Apps being installed
    /// for the first time are not listed until they are.
    pub fn is_installed(&self) -> bool {
        self.state_flags & flags::FULLY_INSTALLED != 0
    }

    /// The update state.
    pub fn update(&self) -> SteamUpdate {
        update_state(self.state_flags)
    }

    /// Download progress while an update is waiting, paused or running.
    pub fn download(&self) -> Option<SteamDownload> {
        (self.update() != SteamUpdate::UpToDate && self.bytes_to_download > 0).then_some(
            SteamDownload {
                done: self.bytes_downloaded.min(self.bytes_to_download),
                total: self.bytes_to_download,
            },
        )
    }

    /// Files missing or damaged.
    pub fn files_missing(&self) -> bool {
        self.state_flags & (flags::FILES_MISSING | flags::FILES_CORRUPT) != 0
    }
}

/// The update state for `StateFlags`: work in progress first, then a pause, then a wait.
pub fn update_state(f: u64) -> SteamUpdate {
    use flags::*;
    let working = UPDATE_RUNNING
        | DOWNLOADING
        | STAGING
        | COMMITTING
        | VALIDATING
        | PREALLOCATING
        | ADDING_FILES
        | RECONFIGURING;
    if f & UPDATE_PAUSED != 0 {
        SteamUpdate::Paused
    } else if f & working != 0 {
        SteamUpdate::Updating
    } else if f & (UPDATE_REQUIRED | UPDATE_STARTED) != 0 {
        SteamUpdate::Waiting
    } else {
        SteamUpdate::UpToDate
    }
}

fn seconds(o: &KvObject, key: &str) -> Option<Timestamp> {
    o.u64(key)
        .filter(|s| *s > 0)
        .and_then(|s| i64::try_from(s).ok())
        .and_then(|s| s.checked_mul(1000))
        .map(Timestamp::from_millis)
}

/// A display name from Steam's text: control and bidirectional-override characters removed,
/// trimmed, at most 80 characters (GAME_LIBRARY §13); `None` when nothing is left.
pub fn clean_name(raw: &str) -> Option<String> {
    let s: String = raw
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.chars().take(crate::consts::GAME_NAME_MAX_CHARS).collect())
}

/// Whether `dir` is a single, plain folder name (no separator, not `.` or `..`).
fn plain_folder_name(dir: &str) -> bool {
    !dir.is_empty() && dir != "." && dir != ".." && !dir.contains('/') && !dir.contains('\0')
}

/// Parse a manifest; `None` when it does not describe an app (no `AppState`, no app ID, no
/// usable install folder).
pub fn parse_manifest(bytes: &[u8]) -> Option<AppManifest> {
    let parsed = parse(bytes, &KvLimits::SMALL);
    let s = parsed.root.obj("AppState")?;
    let app_id = s
        .u64("appid")
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0)?;
    let install_dir = s
        .str("installdir")
        .map(str::trim)
        .filter(|d| plain_folder_name(d))?;
    let name = s
        .str("name")
        .and_then(clean_name)
        .unwrap_or_else(|| format!("Steam app {app_id}"));
    Some(AppManifest {
        app_id,
        name,
        install_dir: install_dir.to_owned(),
        state_flags: s.u64("StateFlags").unwrap_or(0),
        size_on_disk: s.u64("SizeOnDisk").unwrap_or(0),
        build_id: s.u64("buildid").filter(|b| *b > 0),
        last_updated: seconds(s, "LastUpdated"),
        last_played: seconds(s, "LastPlayed"),
        bytes_to_download: s.u64("BytesToDownload").unwrap_or(0),
        bytes_downloaded: s.u64("BytesDownloaded").unwrap_or(0),
    })
}

/// The app ID in a manifest file name (`appmanifest_<appid>.acf`).
pub fn manifest_app_id(file_name: &str) -> Option<u32> {
    file_name
        .strip_prefix("appmanifest_")?
        .strip_suffix(".acf")
        .filter(|d| !d.is_empty() && d.len() <= 10 && d.bytes().all(|b| b.is_ascii_digit()))?
        .parse()
        .ok()
        .filter(|v| *v > 0)
}

// ---- What is not a game -------------------------------------------------------------------

/// Why an installed app is left out of the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotAGame {
    /// A compatibility tool: Proton, or anything with a `toolmanifest.vdf` in its folder.
    CompatibilityTool,
    /// A Steam Linux Runtime (the container Proton and native games run in).
    Runtime,
    /// Shared redistributables (Visual C++, DirectX, .NET, the Steamworks SDK).
    Redistributable,
}

impl NotAGame {
    /// Lower-case text form for the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            NotAGame::CompatibilityTool => "compatibility_tool",
            NotAGame::Runtime => "runtime",
            NotAGame::Redistributable => "redistributable",
        }
    }
}

/// Apps that are never games, by app ID: Valve's Proton builds and runtimes and the shared
/// redistributables (IDs as listed on the apps' store and SteamDB pages).
const KNOWN: &[(u32, NotAGame)] = &[
    (228_980, NotAGame::Redistributable), // Steamworks Common Redistributables
    (1007, NotAGame::Redistributable),    // Steamworks SDK Redist
    (1_070_560, NotAGame::Runtime),       // Steam Linux Runtime 1.0 (scout)
    (1_391_110, NotAGame::Runtime),       // Steam Linux Runtime 2.0 (soldier)
    (1_628_350, NotAGame::Runtime),       // Steam Linux Runtime 3.0 (sniper)
    (4_183_110, NotAGame::Runtime),       // Steam Linux Runtime 4.0
    (4_690_330, NotAGame::Runtime),       // Legacy Steam Runtime
    (858_280, NotAGame::CompatibilityTool), // Proton 3.7
    (930_400, NotAGame::CompatibilityTool), // Proton 3.16
    (961_940, NotAGame::CompatibilityTool), // Proton 4.2
    (1_054_830, NotAGame::CompatibilityTool), // Proton 4.11
    (1_113_280, NotAGame::CompatibilityTool), // Proton 5.0
    (1_245_040, NotAGame::CompatibilityTool), // Proton 5.13
    (1_420_170, NotAGame::CompatibilityTool), // Proton 6.3
    (1_887_720, NotAGame::CompatibilityTool), // Proton 7.0
    (2_348_590, NotAGame::CompatibilityTool), // Proton 8.0
    (2_805_730, NotAGame::CompatibilityTool), // Proton 9.0
    (3_658_110, NotAGame::CompatibilityTool), // Proton 10.0
    (1_493_710, NotAGame::CompatibilityTool), // Proton Experimental
    (2_180_100, NotAGame::CompatibilityTool), // Proton Hotfix
    (1_161_040, NotAGame::CompatibilityTool), // Proton BattlEye Runtime
    (1_826_330, NotAGame::CompatibilityTool), // Proton EasyAntiCheat Runtime
];

/// Classify an app from what is known without its folder: app ID, then name. `None` = a
/// game (as far as this says; [`classify`] also looks at the folder).
pub fn classify_by_id_and_name(app_id: u32, name: &str) -> Option<NotAGame> {
    if let Some((_, why)) = KNOWN.iter().find(|(id, _)| *id == app_id) {
        return Some(*why);
    }
    let n = name.trim().to_ascii_lowercase();
    if n == "proton" || n.starts_with("proton ") || n.starts_with("proton-") {
        return Some(NotAGame::CompatibilityTool);
    }
    if n.starts_with("steam linux runtime") || n == "legacy steam runtime" {
        return Some(NotAGame::Runtime);
    }
    if n.starts_with("steamworks common redistributables")
        || n.starts_with("steamworks sdk redist")
        || n == "steamworks shared"
    {
        return Some(NotAGame::Redistributable);
    }
    None
}

/// Classify an app: known app IDs and names first, then the strongest local sign — a
/// `toolmanifest.vdf` in its install folder, which only compatibility tools and runtimes
/// carry (`has_tool_manifest`).
pub fn classify(app_id: u32, name: &str, has_tool_manifest: bool) -> Option<NotAGame> {
    classify_by_id_and_name(app_id, name)
        .or(has_tool_manifest.then_some(NotAGame::CompatibilityTool))
}

/// `<library>/steamapps/common/<installdir>`.
pub fn install_path(library: &std::path::Path, m: &AppManifest) -> PathBuf {
    library.join("steamapps/common").join(&m.install_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str) -> AppManifest {
        let bytes = match name {
            "1174180" => &include_bytes!("../../tests/data/steam/appmanifest_1174180.acf")[..],
            "12210" => include_bytes!("../../tests/data/steam/appmanifest_12210.acf"),
            "440" => include_bytes!("../../tests/data/steam/appmanifest_440.acf"),
            "228980" => include_bytes!("../../tests/data/steam/appmanifest_228980.acf"),
            "1493710" => include_bytes!("../../tests/data/steam/appmanifest_1493710.acf"),
            _ => include_bytes!("../../tests/data/steam/appmanifest_4183110.acf"),
        };
        parse_manifest(bytes).unwrap()
    }

    #[test]
    fn reads_the_fields() {
        let rdr = m("1174180");
        assert_eq!(rdr.app_id, 1_174_180);
        assert_eq!(rdr.name, "Red Dead Redemption 2");
        assert_eq!(rdr.install_dir, "Red Dead Redemption 2");
        assert_eq!(rdr.size_on_disk, 125_000_000_000);
        assert_eq!(rdr.build_id, Some(13_773_296));
        assert_eq!(
            rdr.last_updated,
            Some(Timestamp::from_millis(1_789_000_000_000))
        );
        assert!(rdr.is_installed());
        assert_eq!(rdr.update(), SteamUpdate::Waiting);
        assert_eq!(
            rdr.download(),
            Some(SteamDownload {
                done: 0,
                total: 148_669_985
            })
        );
        let gta = m("12210");
        assert_eq!(gta.update(), SteamUpdate::UpToDate);
        assert_eq!(gta.last_played, None, "0 means never");
        assert_eq!(gta.download(), None);
        let tf2 = m("440");
        assert_eq!(tf2.update(), SteamUpdate::Paused);
        assert_eq!(tf2.download().unwrap().done, 120_000_000);
        // Fields Steam leaves out.
        let slr = m("4183110");
        assert_eq!(slr.bytes_to_download, 0);
        assert_eq!(slr.update(), SteamUpdate::UpToDate);
    }

    #[test]
    fn state_flags_table() {
        // Values seen on a real machine (TECHNICAL_RESEARCH, ADR-0031).
        for (f, want) in [
            (4, SteamUpdate::UpToDate),
            (6, SteamUpdate::Waiting),
            (518, SteamUpdate::Paused),
            (1542, SteamUpdate::Paused),
            (1574, SteamUpdate::Paused),
            (4 | 256, SteamUpdate::Updating),
            (4 | 2 | (1 << 20), SteamUpdate::Updating),
            (4 | 1024, SteamUpdate::Waiting),
        ] {
            assert_eq!(update_state(f), want, "flags {f}");
        }
        assert!(
            AppManifest {
                state_flags: 1574,
                ..m("12210")
            }
            .files_missing()
        );
    }

    #[test]
    fn rejects_what_is_not_an_app() {
        assert_eq!(parse_manifest(b""), None);
        assert_eq!(
            parse_manifest(b"\"AppState\" { \"appid\" \"0\" \"installdir\" \"x\" }"),
            None
        );
        for dir in ["", "..", ".", "a/b"] {
            let t = format!("\"AppState\" {{ \"appid\" \"5\" \"installdir\" \"{dir}\" }}");
            assert_eq!(parse_manifest(t.as_bytes()), None, "{dir:?}");
        }
        let t = "\"AppState\" { \"appid\" \"5\" \"installdir\" \"x\" \"name\" \"\u{202e}\u{7}A\" }";
        assert_eq!(parse_manifest(t.as_bytes()).unwrap().name, "A");
        let t = "\"AppState\" { \"appid\" \"5\" \"installdir\" \"x\" }";
        assert_eq!(parse_manifest(t.as_bytes()).unwrap().name, "Steam app 5");
    }

    #[test]
    fn manifest_names() {
        assert_eq!(manifest_app_id("appmanifest_440.acf"), Some(440));
        assert_eq!(manifest_app_id("appmanifest_.acf"), None);
        assert_eq!(manifest_app_id("appmanifest_0.acf"), None);
        assert_eq!(manifest_app_id("appmanifest_99999999999.acf"), None);
        assert_eq!(manifest_app_id("appmanifest_44a.acf"), None);
        assert_eq!(manifest_app_id("libraryfolders.vdf"), None);
    }

    #[test]
    fn what_is_not_a_game() {
        assert_eq!(
            classify(228_980, "x", false),
            Some(NotAGame::Redistributable)
        );
        assert_eq!(
            classify(1, "Proton Experimental", false),
            Some(NotAGame::CompatibilityTool)
        );
        assert_eq!(
            classify(1, "Proton 11.0", false),
            Some(NotAGame::CompatibilityTool)
        );
        assert_eq!(
            classify(1, "Steam Linux Runtime 5.0", false),
            Some(NotAGame::Runtime)
        );
        assert_eq!(
            classify(1, "Some Tool", true),
            Some(NotAGame::CompatibilityTool)
        );
        assert_eq!(classify(1_174_180, "Red Dead Redemption 2", false), None);
        // Games whose names merely contain the words stay games.
        assert_eq!(classify(1, "Protonaut", false), None);
        assert_eq!(classify(1, "SteamWorld Dig", false), None);
        // SteamVR and Wallpaper Engine are apps the user starts: left in (ADR-0031 §2).
        assert_eq!(classify(250_820, "SteamVR", false), None);
        assert_eq!(classify(431_960, "Wallpaper Engine", false), None);
    }
}
