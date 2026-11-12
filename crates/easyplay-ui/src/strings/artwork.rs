//! Artwork sheet (SCREEN_SPECS_2 §14, ADR-0018).

/// Slot.
pub const COVER: &str = "Cover";
/// Slot.
pub const ICON: &str = "Icon";
/// Slot.
pub const HERO: &str = "Banner";
/// Button.
pub const CHOOSE: &str = "Choose image…";
/// Accepted formats.
pub const FORMATS: &str = "PNG, JPEG, WebP, BMP or ICO, up to 64 megapixels";
/// Button.
pub const USE_ICON: &str = "Use program icon";
/// Button.
pub const REMOVE: &str = "Remove";
/// Section.
pub const STEAM: &str = "Official artwork";
/// Section body.
pub const STEAM_BODY: &str = "EasyPlay can download this game's cover from Steam's image server. Only the Steam app ID is sent.";
/// Shown while online artwork is off.
pub const STEAM_OFF: &str =
    "Downloading official artwork is turned off. You can turn it on in Settings → General.";
/// Field.
pub const APP_ID: &str = "Steam app ID or store link";
/// Field hint.
pub const APP_ID_HINT: &str = "e.g. 1145360";
/// Field error.
pub const APP_ID_ERROR: &str = "Enter a number or a store.steampowered.com link.";
/// Button.
pub const FETCH: &str = "Download official artwork";

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
