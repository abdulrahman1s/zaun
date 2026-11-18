//! Chat strings: profile pictures (ADR-0038; chat-calls.html C and table S). Keys live under
//! `chat.profile.` in locales/en.yml and ar.yml, between that file's `chat.profile` markers.
//! Why a file could not be used comes from the error catalogue, not from here.

texts! {
    /// Opens the file chooser.
    CHOOSE = "chat.profile.choose_picture";
    /// Its tooltip: what is accepted and what friends get.
    LIMITS = "chat.profile.picture_limits";
    /// Back to initials (sent on Save).
    REMOVE = "chat.profile.remove_picture";
    /// While the file is read.
    PREPARING = "chat.profile.preparing";
    /// The crop step's title.
    CROP_TITLE = "chat.profile.crop_title";
    /// Under it.
    CROP_TEXT = "chat.profile.crop_text";
    /// The crop area's accessible name (what its keys do).
    CROP_AREA = "chat.profile.crop_area";
    /// The zoom slider's name.
    ZOOM = "chat.profile.zoom";
    /// The crop step's main action ("Save" is the profile's).
    USE_PICTURE = "chat.profile.use_picture";
    /// A group's picture: the dialog's title, the details row's label.
    GROUP_TITLE = "chat.profile.group_title";
    /// Under it.
    GROUP_TEXT = "chat.profile.group_text";
    /// The details row's label.
    GROUP_ROW = "chat.profile.group_row";
    /// The details row's value.
    CHANGE = "chat.profile.change";
}

/// The zoom as the slider speaks it ("150 %").
pub fn zoom_value(percent: u32) -> String {
    rust_i18n::t!("chat.profile.zoom_value", n = percent).into_owned()
}
