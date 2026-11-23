//! HPACK request encoding: static-table matches become indexed fields, the rest plain literals (RFC 7541 section 6).
use super::hpack_int::encode_int;
use super::static_table::STATIC_TABLE;

fn push_string(s: &str, out: &mut Vec<u8>) {
    encode_int(s.len(), 7, 0x00, out);
    out.extend_from_slice(s.as_bytes());
}

/// Literal fields are written without indexing and without Huffman coding.
#[must_use]
pub fn encode_headers(headers: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(name, value) in headers {
        let name_idx = STATIC_TABLE.iter().position(|(n, _)| *n == name);
        let full = STATIC_TABLE
            .iter()
            .position(|e| *e == (name, value) && !value.is_empty());
        match (full, name_idx) {
            (Some(i), _) => encode_int(i + 1, 7, 0x80, &mut out),
            (None, Some(i)) => {
                encode_int(i + 1, 4, 0x00, &mut out);
                push_string(value, &mut out);
            }
            (None, None) => {
                out.push(0x00);
                push_string(name, &mut out);
                push_string(value, &mut out);
            }
        }
    }
    out
}
