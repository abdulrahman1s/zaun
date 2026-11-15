//! Artwork sheet (SCREEN_SPECS_2 §14, ADR-0018, ADR-0024).

use easyplay_core::artwork::SteamAppIdSource;

/// Slot.
pub const COVER: &str = "Cover";
/// Slot.
pub const ICON: &str = "Icon";
/// Slot.
pub const HERO: &str = "Banner";
/// Button under a slot.
pub const CHOOSE: &str = "Choose…";
/// Under the slots, once for all three.
pub const HOW_TO_CHANGE: &str = "Click a picture to choose an image, or drop one on it. PNG, JPEG, WebP, BMP or ICO, up to 64 megapixels.";
/// Button under the icon.
pub const USE_ICON: &str = "Use program icon";
/// Button under a slot.
pub const REMOVE: &str = "Remove";
/// Section.
pub const STEAM: &str = "Official artwork from Steam";
/// Section body: what is sent, and that the game's own permissions do not matter.
pub const STEAM_BODY: &str = "EasyPlay downloads it itself and sends Steam only the app ID or the words you search for. The game itself stays offline.";
/// Shown while online artwork is off.
pub const STEAM_OFF: &str = "Downloading official artwork is turned off. EasyPlay would send Steam only the app ID or the game's name; the game itself stays offline.";
/// Button that turns online artwork on.
pub const TURN_ON: &str = "Turn on";
/// Search field label.
pub const SEARCH_LABEL: &str = "Game name, Steam app ID or store link";
/// Search field hint.
pub const SEARCH_HINT: &str = "e.g. Hollow Knight, 367520";
/// Button.
pub const SEARCH: &str = "Search";
/// While the lookup runs.
pub const SEARCHING: &str = "Looking on Steam…";
/// Nothing found.
pub const NOTHING_FOUND: &str = "Nothing found on Steam. Try fewer words, or paste the game's Steam store link. Games no longer sold on Steam are found only by their app ID or link.";
/// Name search is turned off in Settings.
pub const NAME_SEARCH_OFF: &str =
    "Searching by name is turned off in Settings. Enter the Steam app ID or store link.";
/// The lookup failed without a message.
pub const LOOKUP_FAILED: &str = "Steam could not be reached. Try again.";
/// Row value.
pub const DOWNLOAD: &str = "Download artwork";
/// Row value while it runs.
pub const DOWNLOADING: &str = "downloading…";
/// Row value of the app whose artwork is in use.
pub const IN_USE: &str = "in use";

/// "Artwork for Hollow Knight".
pub fn title(name: &str) -> String {
    format!("Artwork for {name}")
}

/// "Cover: change, current image set".
pub fn slot_name(slot: &str, has: bool) -> String {
    if has {
        format!("{slot}: change. An image is set.")
    } else {
        format!("{slot}: change. No image yet.")
    }
}

/// "Steam app 367520".
pub fn app(id: u32) -> String {
    format!("Steam app {id}")
}

/// Where the computer found the app ID.
pub fn found_in(source: SteamAppIdSource) -> &'static str {
    match source {
        SteamAppIdSource::Stored | SteamAppIdSource::User => "Used for this game before",
        SteamAppIdSource::AppIdFile => "Named by a file in the game folder",
        SteamAppIdSource::UmuDb => "Known to the game-fix database",
        SteamAppIdSource::NameSearch => "Found by its name",
    }
}

/// Accessible name of a result row.
pub fn row_name(title: &str, id: u32) -> String {
    format!("{title}, Steam app {id}: download its artwork")
}
