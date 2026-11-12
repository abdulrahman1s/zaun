//! Tag editor.

/// No tags yet.
pub const NONE: &str = "No tags yet.";
/// Field label.
pub const ADD: &str = "Add a tag";
/// Field hint.
pub const HINT: &str = "Type a tag name…";
/// Footer.
pub const DONE: &str = "Done";

/// "Tags for Hollow Knight".
pub fn title(name: &str) -> String {
    format!("Tags for {name}")
}

/// "Create “speedrun”".
pub fn create(name: &str) -> String {
    format!("Create “{name}”")
}
