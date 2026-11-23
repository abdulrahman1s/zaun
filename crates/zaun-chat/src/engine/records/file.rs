//! What a file item needs beyond its item: direction, local copy, the friend's description as it arrives (Store/Files.hs `RcvFileTransfer`, `appendRcvFD`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use zaun_chat_wire::FileKind;

/// `rcvFileDescrWithinLimits`: a description of at most this many parts and characters.
const MAX_PARTS: u32 = 1024;
const MAX_CHARS: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Way {
    Sent,
    Received,
}

/// A part that does not follow the ones before it (`SERcvFileInvalidDescrPart`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadPart;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub way: Way,
    pub kind: FileKind,
    /// The copy on this computer: the file sent, or the file received.
    pub local: Option<PathBuf>,
    /// A transfer was running when this was last saved; a start finding it set ends it.
    pub active: bool,
    /// Received: the user asked for it before the whole description was here.
    pub accepted: bool,
    /// Received: the sender cancelled, so there is nothing left to fetch.
    pub sender_cancelled: bool,
    /// Received: the description text so far, and the number of the last part added.
    pub descr: String,
    pub last_part: u32,
    pub descr_complete: bool,
}

impl FileRecord {
    #[must_use]
    pub fn new(way: Way, kind: FileKind, local: Option<PathBuf>) -> Self {
        Self {
            way,
            kind,
            local,
            active: false,
            accepted: false,
            sender_cancelled: false,
            descr: String::new(),
            last_part: 0,
            descr_complete: false,
        }
    }

    /// Adds the next part, as `appendRcvFD` does: it must follow the one before (the offer
    /// itself counts as part 0), and the text stays within limits.
    pub fn append(&mut self, text: &str, part_no: u32, complete: bool) -> Result<(), BadPart> {
        let chars = self.descr.chars().count() + text.chars().count();
        if self.descr_complete
            || part_no != self.last_part + 1
            || part_no > MAX_PARTS
            || chars > MAX_CHARS
        {
            return Err(BadPart);
        }
        self.descr.push_str(text);
        (self.last_part, self.descr_complete) = (part_no, complete);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> FileRecord {
        FileRecord::new(Way::Received, FileKind::File, None)
    }

    #[test]
    fn parts_must_follow_each_other_and_stop_at_the_last() {
        let mut r = rec();
        assert!(r.append("ab", 1, false).is_ok());
        assert_eq!(r.append("x", 3, false), Err(BadPart));
        assert!(r.append("cd", 2, true).is_ok());
        assert_eq!((r.descr.as_str(), r.descr_complete), ("abcd", true));
        assert_eq!(r.append("e", 3, true), Err(BadPart));
    }

    #[test]
    fn a_description_over_the_limits_is_refused() {
        let mut r = rec();
        assert!(r.append(&"a".repeat(MAX_CHARS), 1, false).is_ok());
        assert_eq!(r.append("b", 2, true), Err(BadPart));
    }
}
