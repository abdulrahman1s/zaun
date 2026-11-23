//! The header inside an encrypted file: its name and an extension slot (Types.hs `FileHeader`).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

/// Longest name `Text`'s one-byte length prefix can carry.
const MAX_NAME: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    pub file_name: String,
    pub file_extra: Option<String>,
}

fn clip(s: &str) -> &str {
    let mut end = s.len().min(MAX_NAME);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

impl FileHeader {
    /// `smpEncode (fileName, fileExtra)`; a name over 255 bytes is cut at a character boundary.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        clip(&self.file_name).encode(&mut out)?;
        self.file_extra
            .as_deref()
            .map(clip)
            .map(str::to_owned)
            .encode(&mut out)?;
        Ok(out)
    }

    /// Reads the header off the front of decrypted content; returns it with its length.
    pub fn parse(content: &[u8]) -> Result<(Self, usize), DecodeError> {
        let mut d = Decoder::new(&content[..content.len().min(1024)]);
        let file_name = String::decode(&mut d)?;
        let file_extra = Option::<String>::decode(&mut d)?;
        Ok((
            Self {
                file_name,
                file_extra,
            },
            d.offset(),
        ))
    }
}
