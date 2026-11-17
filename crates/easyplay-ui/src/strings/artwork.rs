//! Artwork sheet (SCREEN_SPECS_2 §14, ADR-0018, ADR-0024).

use easyplay_core::artwork::SteamAppIdSource;

texts! {
    /// Slot.
    COVER = "artwork.cover";
    /// Slot.
    ICON = "artwork.icon";
    /// Slot.
    HERO = "artwork.hero";
    /// Button under a slot.
    CHOOSE = "artwork.choose";
    /// Under the slots, once for all three.
    HOW_TO_CHANGE = "artwork.how_to_change";
    /// Help under "Cover" while the cover is EasyPlay's own poster.
    MADE_FROM_TITLE = "artwork.made_from_title";
    /// The dialog's action.
    DONE = "artwork.done";
    /// Button under the icon.
    USE_ICON = "artwork.use_icon";
    /// Button under a slot.
    REMOVE = "artwork.remove";
    /// Section.
    STEAM = "artwork.steam";
    /// Section body: what is sent, and that the game's own permissions do not matter.
    STEAM_BODY = "artwork.steam_body";
    /// Shown while online artwork is off.
    STEAM_OFF = "artwork.steam_off";
    /// Button that turns online artwork on.
    TURN_ON = "artwork.turn_on";
    /// Search field label.
    SEARCH_LABEL = "artwork.search_label";
    /// Search field hint.
    SEARCH_HINT = "artwork.search_hint";
    /// Button.
    SEARCH = "artwork.search";
    /// While the lookup runs.
    SEARCHING = "artwork.searching";
    /// Nothing found.
    NOTHING_FOUND = "artwork.nothing_found";
    /// Name search is turned off in Settings.
    NAME_SEARCH_OFF = "artwork.name_search_off";
    /// The lookup failed without a message.
    LOOKUP_FAILED = "artwork.lookup_failed";
    /// Row value.
    DOWNLOAD = "artwork.download";
    /// Row value while it runs.
    DOWNLOADING = "artwork.downloading";
    /// Row value of the app whose artwork is in use.
    IN_USE = "artwork.in_use";
}

/// What is sent to Steam, and that the game itself stays where its protection keeps it.
pub fn steam_body(offline: bool) -> &'static str {
    if offline {
        STEAM_BODY()
    } else {
        crate::i18n::text("artwork.steam_body_online")
    }
}

texts! {
    /// The cover preview at the dialog's left, which chooses a cover too.
    COVER_PREVIEW = "artwork.cover_preview";
    /// What the cover preview does.
    PREVIEW_HINT = "artwork.preview_hint";
}

/// "Artwork for Hollow Knight".
pub fn title(name: &str) -> String {
    rust_i18n::t!("artwork.title", name = name).into_owned()
}

/// "Cover: change, current image set".
pub fn slot_name(slot: &str, has: bool) -> String {
    if has {
        rust_i18n::t!("artwork.slot_name_set", slot = slot).into_owned()
    } else {
        rust_i18n::t!("artwork.slot_name_empty", slot = slot).into_owned()
    }
}

/// "Steam app 367520".
pub fn app(id: u32) -> String {
    rust_i18n::t!("artwork.app", id = id).into_owned()
}

/// Where the computer found the app ID.
pub fn found_in(source: SteamAppIdSource) -> &'static str {
    match source {
        SteamAppIdSource::Stored | SteamAppIdSource::User => {
            crate::i18n::text("artwork.found_in.used_for_this_game")
        }
        SteamAppIdSource::AppIdFile => crate::i18n::text("artwork.found_in.named_by_a_file"),
        SteamAppIdSource::UmuDb => crate::i18n::text("artwork.found_in.known_to_the_game"),
        SteamAppIdSource::NameSearch => crate::i18n::text("artwork.found_in.found_by_its_name"),
    }
}

/// Accessible name of a result row.
pub fn row_name(title: &str, id: u32) -> String {
    rust_i18n::t!("artwork.row_name", title = title, id = id).into_owned()
}
