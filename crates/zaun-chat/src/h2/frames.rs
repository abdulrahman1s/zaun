//! HTTP/2 frame layout and the frames a client builds (RFC 9113 sections 4, 6).

pub const DATA: u8 = 0x0;
pub const HEADERS: u8 = 0x1;
pub const PRIORITY: u8 = 0x2;
pub const RST_STREAM: u8 = 0x3;
pub const SETTINGS: u8 = 0x4;
pub const PUSH_PROMISE: u8 = 0x5;
pub const PING: u8 = 0x6;
pub const GOAWAY: u8 = 0x7;
pub const WINDOW_UPDATE: u8 = 0x8;
pub const CONTINUATION: u8 = 0x9;

pub const END_STREAM: u8 = 0x1;
pub const ACK: u8 = 0x1;
pub const END_HEADERS: u8 = 0x4;
pub const PADDED: u8 = 0x8;
pub const PRIORITY_FLAG: u8 = 0x20;

pub const SETTINGS_HEADER_TABLE_SIZE: u16 = 0x1;
pub const SETTINGS_ENABLE_PUSH: u16 = 0x2;
pub const SETTINGS_INITIAL_WINDOW_SIZE: u16 = 0x4;
pub const SETTINGS_MAX_FRAME_SIZE: u16 = 0x5;

pub const NO_ERROR: u32 = 0x0;
pub const PROTOCOL_ERROR: u32 = 0x1;
pub const CANCEL: u32 = 0x8;

pub const DEFAULT_WINDOW: i64 = 65535;
pub const MAX_WINDOW: i64 = (1 << 31) - 1;
pub const DEFAULT_MAX_FRAME: usize = 16384;
pub const HEADER_LEN: usize = 9;

pub const CLIENT_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub length: usize,
    pub kind: u8,
    pub flags: u8,
    pub stream: u32,
}

pub fn append_frame(out: &mut Vec<u8>, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
    let len = payload.len();
    out.extend_from_slice(&[(len >> 16) as u8, (len >> 8) as u8, len as u8, kind, flags]);
    out.extend_from_slice(&(stream & 0x7fff_ffff).to_be_bytes());
    out.extend_from_slice(payload);
}

#[must_use]
pub fn encode_frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    append_frame(&mut out, kind, flags, stream, payload);
    out
}

#[must_use]
pub fn parse_header(b: &[u8; HEADER_LEN]) -> FrameHeader {
    FrameHeader {
        length: usize::from(b[0]) << 16 | usize::from(b[1]) << 8 | usize::from(b[2]),
        kind: b[3],
        flags: b[4],
        stream: u32::from_be_bytes([b[5], b[6], b[7], b[8]]) & 0x7fff_ffff,
    }
}

#[must_use]
pub fn encode_settings(settings: &[(u16, u32)]) -> Vec<u8> {
    let payload: Vec<u8> = settings
        .iter()
        .flat_map(|(id, v)| [id.to_be_bytes().as_slice(), v.to_be_bytes().as_slice()].concat())
        .collect();
    encode_frame(SETTINGS, 0, 0, &payload)
}

/// `None` when the payload is not a whole number of 6-byte entries.
#[must_use]
pub fn decode_settings(p: &[u8]) -> Option<Vec<(u16, u32)>> {
    let (entries, rest) = p.as_chunks::<6>();
    rest.is_empty().then(|| {
        entries
            .iter()
            .map(|c| {
                (
                    u16::from_be_bytes([c[0], c[1]]),
                    u32::from_be_bytes([c[2], c[3], c[4], c[5]]),
                )
            })
            .collect()
    })
}

#[must_use]
pub fn encode_window_update(stream: u32, increment: u32) -> Vec<u8> {
    encode_frame(
        WINDOW_UPDATE,
        0,
        stream,
        &(increment & 0x7fff_ffff).to_be_bytes(),
    )
}

#[must_use]
pub fn encode_rst_stream(stream: u32, code: u32) -> Vec<u8> {
    encode_frame(RST_STREAM, 0, stream, &code.to_be_bytes())
}

#[must_use]
pub fn encode_goaway(last_stream: u32, code: u32) -> Vec<u8> {
    let payload = [
        (last_stream & 0x7fff_ffff).to_be_bytes(),
        code.to_be_bytes(),
    ]
    .concat();
    encode_frame(GOAWAY, 0, 0, &payload)
}

#[must_use]
pub fn encode_ping(data: [u8; 8], ack: bool) -> Vec<u8> {
    encode_frame(PING, if ack { ACK } else { 0 }, 0, &data)
}
