//! The binary `F` element (Protocol.hs `AppMessageBinary`, `BFileChunk`, `InlineFileChunk`).
use super::types::{ChatEvent, FileChunk};
use crate::chat::bin::{Reader, put_short};
use crate::chat::error::{Result, parse_err};
use crate::chat::ids::SharedMsgId;

/// `'F'`, an empty outer msgId, then the file's msgId and the chunk number byte (`\0` is a cancel) plus bytes.
pub(crate) fn encode_chunk(msg_id: &SharedMsgId, chunk: &FileChunk) -> Result<Vec<u8>> {
    let mut out = vec![b'F', 0];
    put_short(&mut out, msg_id.as_bytes())?;
    match chunk {
        FileChunk::Chunk { chunk_no, bytes } => {
            out.push(*chunk_no);
            out.extend_from_slice(bytes);
        }
        FileChunk::Cancel => out.push(0),
    }
    Ok(out)
}

pub(crate) fn parse_chunk(element: &[u8]) -> Result<ChatEvent> {
    let mut r = Reader(element);
    if r.u8()? != b'F' {
        return parse_err("not a file chunk");
    }
    let _outer_msg_id = r.short_bytes()?;
    let msg_id = SharedMsgId(r.short_bytes()?.to_vec());
    let chunk = match r.u8()? {
        0 => FileChunk::Cancel,
        chunk_no => FileChunk::Chunk {
            chunk_no,
            bytes: r.rest().to_vec(),
        },
    };
    Ok(ChatEvent::FileChunk { msg_id, chunk })
}
