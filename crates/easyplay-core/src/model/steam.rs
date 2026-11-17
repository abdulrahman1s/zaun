//! Steam games in the library (ADR-0031, GAME_LIBRARY §15): where a library entry comes from,
//! what Steam says about it, and the states screens show in place of a protection preset.
//!
//! Everything in [`SteamFacts`] is read again from Steam's own files on every scan and is
//! never stored by EasyPlay.

use std::path::PathBuf;
use std::sync::Arc;

use super::{GameSummary, Preset};
use crate::clock::Timestamp;

/// How Steam is installed on this computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SteamInstallKind {
    /// The distribution's or Valve's package (`~/.local/share/Steam`, `~/.steam/steam`).
    Native,
    /// The Flathub package `com.valvesoftware.Steam`.
    Flatpak,
    /// The Snap package `steam`.
    Snap,
}

impl SteamInstallKind {
    /// Lower-case text form (`native`, `flatpak`, `snap`).
    pub const fn as_str(self) -> &'static str {
        match self {
            SteamInstallKind::Native => "native",
            SteamInstallKind::Flatpak => "flatpak",
            SteamInstallKind::Snap => "snap",
        }
    }
}

/// Whether Steam has an update for the game, read from the manifest's `StateFlags`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SteamUpdate {
    /// Installed, nothing waiting.
    #[default]
    UpToDate,
    /// An update is waiting (queued or scheduled by Steam).
    Waiting,
    /// An update was started and is paused.
    Paused,
    /// Steam is downloading or applying an update now.
    Updating,
}

impl SteamUpdate {
    /// Lower-case text form for the command line and logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            SteamUpdate::UpToDate => "up_to_date",
            SteamUpdate::Waiting => "waiting",
            SteamUpdate::Paused => "paused",
            SteamUpdate::Updating => "updating",
        }
    }
}

/// Download progress of an update, from the manifest (bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SteamDownload {
    /// Bytes downloaded so far.
    pub done: u64,
    /// Bytes to download in total.
    pub total: u64,
}

/// Whether Steam runs the game through Proton.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SteamCompat {
    /// Steam's files show no Proton use for this game: a Linux version, or a Windows game
    /// that has not been played yet with Steam's default compatibility tool.
    NotProton,
    /// Steam runs it through Proton.
    Proton {
        /// The compatibility tool Steam was told to use for this game (its internal name,
        /// e.g. `proton_experimental`, `GE-Proton10-25`); `None` = Steam's default.
        tool: Option<String>,
        /// The Proton version that last ran the prefix (`compatdata/<appid>/version`).
        version: Option<String>,
        /// `steamapps/compatdata/<appid>` in the library folder that holds the game.
        compat_data: PathBuf,
        /// The Windows storage `compat_data/pfx`, when it exists.
        prefix: Option<PathBuf>,
        /// The Proton directory that last ran the prefix (`compatdata/<appid>/config_info`),
        /// when it still exists.
        proton_dir: Option<PathBuf>,
    },
}

/// A request for the Steam client.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SteamRequest {
    /// Start a game.
    Run(u32),
    /// Show the downloads page ("Update in Steam").
    Downloads,
    /// Show a game in Steam's library ("Open in Steam").
    Details(u32),
}

impl SteamRequest {
    /// The `steam://` URL (digits only besides the fixed text).
    pub fn url(self) -> String {
        match self {
            SteamRequest::Run(id) => format!("steam://rungameid/{id}"),
            SteamRequest::Downloads => "steam://open/downloads".to_owned(),
            SteamRequest::Details(id) => format!("steam://nav/games/details/{id}"),
        }
    }
}

/// What Steam's own files say about one installed game.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SteamFacts {
    /// The app ID: the game's identity.
    pub app_id: u32,
    /// Which Steam installation lists it.
    pub install: SteamInstallKind,
    /// The Steam library folder (the folder holding `steamapps`).
    pub library_folder: PathBuf,
    /// `steamapps/common/<installdir>`.
    pub install_dir: PathBuf,
    /// Size on disk in bytes (`SizeOnDisk`).
    pub size_on_disk: u64,
    /// Installed build (`buildid`).
    pub build_id: Option<u64>,
    /// When Steam last updated it (`LastUpdated`).
    pub last_updated: Option<Timestamp>,
    /// Update state.
    pub update: SteamUpdate,
    /// Progress of a waiting, paused or running update, when Steam reports one.
    pub download: Option<SteamDownload>,
    /// Steam marked files as missing or damaged (it repairs them when the game updates or
    /// is verified).
    pub files_missing: bool,
    /// Proton use.
    pub compat: SteamCompat,
    /// Play time Steam recorded for the most recently signed-in account, in minutes; `None`
    /// when Steam's files do not say.
    pub playtime_minutes: Option<u64>,
}

impl SteamFacts {
    /// Whether the game has a Windows storage EasyPlay can offer actions for.
    pub fn prefix(&self) -> Option<&std::path::Path> {
        match &self.compat {
            SteamCompat::Proton {
                prefix: Some(p), ..
            } => Some(p),
            _ => None,
        }
    }
}

/// Where a library entry comes from (ADR-0031).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameOrigin {
    /// Added to EasyPlay; runs in EasyPlay's protected space.
    #[default]
    EasyPlay,
    /// Installed with Steam and run by Steam.
    Steam(Arc<SteamFacts>),
}

/// Where a displayed play time comes from (STATISTICS §11). Time is never counted twice: a
/// Steam game's time is Steam's total, an EasyPlay game's time is EasyPlay's sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaytimeSource {
    /// EasyPlay's own recorded sessions.
    EasyPlay,
    /// Steam's total for the most recently signed-in Steam account.
    Steam,
}

/// What protects a game (SECURITY_MODEL §10): an EasyPlay preset, or nothing EasyPlay
/// controls because Steam runs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProtectionState {
    /// EasyPlay runs it with this preset.
    Preset(Preset),
    /// Steam runs it, outside EasyPlay's protected space; no preset applies and none can be
    /// chosen.
    RunBySteam,
}

impl GameSummary {
    /// The Steam facts of a Steam game.
    pub fn steam(&self) -> Option<&SteamFacts> {
        match &self.origin {
            GameOrigin::Steam(f) => Some(f),
            GameOrigin::EasyPlay => None,
        }
    }

    /// Whether Steam runs this game.
    pub fn is_steam(&self) -> bool {
        matches!(self.origin, GameOrigin::Steam(_))
    }

    /// What protects it. Screens show this, never [`GameSummary::preset`] directly: a Steam
    /// game's `preset` is a placeholder without meaning.
    pub fn protection(&self) -> ProtectionState {
        if self.is_steam() {
            ProtectionState::RunBySteam
        } else {
            ProtectionState::Preset(self.preset)
        }
    }

    /// Where [`GameSummary::total_playtime_ms`] and [`GameSummary::last_played_at`] come from.
    pub fn playtime_source(&self) -> PlaytimeSource {
        if self.is_steam() {
            PlaytimeSource::Steam
        } else {
            PlaytimeSource::EasyPlay
        }
    }

    /// "Not played yet": no counted session (EasyPlay), or no time and no last-played date
    /// (Steam).
    pub fn never_played(&self) -> bool {
        if self.is_steam() {
            self.total_playtime_ms == 0 && self.last_played_at.is_none()
        } else {
            self.session_count == 0
        }
    }
}
