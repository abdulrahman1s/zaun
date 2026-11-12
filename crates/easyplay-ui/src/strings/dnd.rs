//! Drag and drop (USER_EXPERIENCE §5.4).

use crate::input::dnd::Payload;

/// Drop zone title.
pub const DROP_TO_ADD: &str = "Drop to add to your library";
/// Refused payload.
pub const REFUSED: &str = "EasyPlay can't open this kind of file";

/// "1 program", "2 folders"; `None` while unknown.
pub fn payload(p: Payload) -> Option<String> {
    let n = |n: usize, one: &str, many: &str| format!("{n} {}", super::plural(n as u64, one, many));
    match p {
        Payload::Programs(c) => Some(n(c, "program", "programs")),
        Payload::Folders(c) => Some(n(c, "folder", "folders")),
        Payload::Images(c) => Some(n(c, "image", "images")),
        Payload::Unsupported | Payload::Unknown => None,
    }
}
