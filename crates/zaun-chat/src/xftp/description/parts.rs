//! A description as chat messages: `x.msg.file.descr` parts (simplex-chat Library/Internal.hs `splitFileDescr`, Store/Files.hs `appendRcvFD`).

/// Haskell's `xftpDescrPartSize`.
pub const DESCR_PART_SIZE: usize = 14000;
/// Room kept on the last part for the badge a sender may attach (`badgeDescrPartSize`).
pub const BADGE_DESCR_PART_SIZE: usize = 13500;
const MAX_RCV_PARTS: u32 = 1024;
const MAX_RCV_TEXT_CHARS: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescrPart {
    pub text: String,
    /// From 1; part 0 is the empty placeholder in the file invitation.
    pub part_no: u32,
    pub complete: bool,
}

/// Cuts `text` by characters, not bytes: parts of `part_size`, and a last part of at most `last_size`.
#[must_use]
pub fn split_description(text: &str, part_size: usize, last_size: usize) -> Vec<DescrPart> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let mut at = 0;
    for part_no in 1.. {
        let n = chars.len() - at;
        let take = if n <= last_size {
            n
        } else if n <= part_size {
            last_size
        } else {
            part_size
        };
        let complete = at + take >= chars.len();
        parts.push(DescrPart {
            text: chars[at..at + take].iter().collect(),
            part_no,
            complete,
        });
        at += take;
        if complete {
            break;
        }
    }
    parts
}

/// Receives parts in order, as `appendRcvFD` does.
#[derive(Debug, Default)]
pub struct DescrAssembler {
    text: String,
    last_no: u32,
    complete: bool,
}

impl DescrAssembler {
    /// The whole text when `part` was the last one.
    pub fn add(&mut self, part: &DescrPart) -> Result<Option<String>, String> {
        let text = format!("{}{}", self.text, part.text);
        let within = part.part_no <= MAX_RCV_PARTS && text.chars().count() <= MAX_RCV_TEXT_CHARS;
        if part.part_no != self.last_no + 1 || self.complete || !within {
            return Err("invalid file description part".into());
        }
        (self.text, self.last_no, self.complete) = (text, part.part_no, part.complete);
        Ok(self.complete.then(|| self.text.clone()))
    }
}

pub fn join_description(parts: &[DescrPart]) -> Result<String, String> {
    let mut sorted: Vec<&DescrPart> = parts.iter().collect();
    sorted.sort_by_key(|p| p.part_no);
    let mut assembler = DescrAssembler::default();
    let mut whole = None;
    for p in sorted {
        whole = assembler.add(p)?;
    }
    whole.ok_or_else(|| "incomplete file description".into())
}
