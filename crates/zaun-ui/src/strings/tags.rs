//! Tags: the tag menu and the tag dialog.

texts! {
    /// Footer.
    DONE = "tags.done";
    /// The field at the end of the tag menu (placeholder).
    NEW_TAG = "tags.new_tag";
    /// The field's accessible name.
    NEW_TAG_NAME = "tags.new_tag_name";
}

/// "Tags for Hollow Knight".
pub fn title(name: &str) -> String {
    rust_i18n::t!("tags.title", name = name).into_owned()
}
