//! Drag and drop (USER_EXPERIENCE §5.4).

use crate::input::dnd::Payload;

texts! {
    /// Drop zone title.
    DROP_TO_ADD = "dnd.drop_to_add";
    /// Refused payload.
    REFUSED = "dnd.refused";
}

/// "1 program", "2 folders"; `None` while unknown.
pub fn payload(p: Payload) -> Option<String> {
    match p {
        Payload::Programs(c) => Some(crate::i18n::plural::counted("dnd.programs", c as u64)),
        Payload::Folders(c) => Some(crate::i18n::plural::counted("dnd.folders", c as u64)),
        Payload::Images(c) => Some(crate::i18n::plural::counted("dnd.images", c as u64)),
        Payload::Unsupported | Payload::Unknown => None,
    }
}
