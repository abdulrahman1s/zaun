//! Reading a description from its YAML text (Description.hs `StrEncoding AFileDescription`, `decodeFileDescription`).
use crate::encoding::b64_decode;
use crate::xftp::address::XftpServer;

use super::file_size::decode_file_size;
use super::model::{FileDescription, Party, Redirect};
use super::replicas::{decode_replica, fold};
use super::yaml_node::Node;
use super::yaml_parse::parse;

/// Hex digits in a row that make the Haskell reader refuse a description: a guard against huge numbers.
const MAX_HEX_RUN: usize = 64;

fn refuse_unsafe(text: &str) -> Result<(), String> {
    if text.contains('*') {
        return Err("YAML aliases are not allowed".into());
    }
    let mut run = 0usize;
    for c in text.chars() {
        run = if c.is_ascii_hexdigit() { run + 1 } else { 0 };
        if run > MAX_HEX_RUN {
            return Err("number too long in file description".into());
        }
    }
    Ok(())
}

fn field<'a>(root: &'a Node, key: &str) -> Result<&'a str, String> {
    root.get(key)
        .and_then(Node::scalar)
        .ok_or_else(|| format!("file description has no {key}"))
}

fn bytes(root: &Node, key: &str) -> Result<Vec<u8>, String> {
    b64_decode(field(root, key)?).map_err(|_| format!("bad base64 in {key}"))
}

fn size32(s: &str) -> Result<u32, String> {
    decode_file_size(s).and_then(|n| u32::try_from(n).map_err(|_| format!("bad chunk size: {s}")))
}

impl FileDescription {
    /// Reads a description without checking its limits; `validate` does that.
    pub fn parse(text: &str) -> Result<Self, String> {
        refuse_unsafe(text)?;
        let root = parse(text)?;
        let party = match field(&root, "party")? {
            "recipient" => Party::Recipient,
            "sender" => Party::Sender,
            other => return Err(format!("bad party: {other}")),
        };
        let chunk_size = size32(field(&root, "chunkSize")?)?;
        let mut flat = Vec::new();
        for group in root.get("replicas").map_or(&[][..], Node::items) {
            let server: XftpServer = field(group, "server")?
                .parse()
                .map_err(|e| format!("bad server: {e}"))?;
            for chunk in group.get("chunks").map_or(&[][..], Node::items) {
                flat.push(decode_replica(
                    &server,
                    chunk.scalar().ok_or("bad chunk replica")?,
                )?);
            }
        }
        let redirect = root
            .get("redirect")
            .map(|r| {
                let size = field(r, "size")?
                    .parse::<u64>()
                    .map_err(|_| "bad redirect size".to_owned())?;
                Ok::<_, String>(Redirect {
                    size,
                    digest: bytes(r, "digest")?,
                })
            })
            .transpose()?;
        Ok(Self {
            party,
            size: decode_file_size(field(&root, "size")?)?,
            digest: bytes(&root, "digest")?,
            key: bytes(&root, "key")?,
            nonce: bytes(&root, "nonce")?,
            chunk_size,
            chunks: fold(chunk_size, flat)?,
            redirect,
        })
    }

    /// `strDecode` of a `ValidFileDescription`.
    pub fn parse_valid(text: &str) -> Result<Self, String> {
        let fd = Self::parse(text)?;
        fd.validate()?;
        Ok(fd)
    }
}
