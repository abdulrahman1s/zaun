//! HPACK response decoding with the dynamic table (RFC 7541 sections 2.3, 3, 4, 6).
use std::collections::VecDeque;

use super::hpack_int::{HpackError, decode_int};
use super::huffman::Huffman;
use super::static_table::STATIC_TABLE;

pub type Headers = Vec<(String, String)>;

const ENTRY_OVERHEAD: usize = 32;

pub struct HpackDecoder {
    /// Newest first; bounded by `max_size` octets (entries evicted from the back).
    dynamic: VecDeque<(String, String)>,
    dynamic_size: usize,
    max_size: usize,
    limit: usize,
    max_header_bytes: usize,
    huffman: Huffman,
}

fn entry_size(name: &str, value: &str) -> usize {
    name.len() + value.len() + ENTRY_OVERHEAD
}

fn latin1(raw: &[u8]) -> String {
    raw.iter().map(|&b| char::from(b)).collect()
}

impl HpackDecoder {
    /// `limit` is the table size we announced (`SETTINGS_HEADER_TABLE_SIZE`), which the peer may not exceed.
    #[must_use]
    pub fn new(limit: usize, max_header_bytes: usize) -> Self {
        Self {
            dynamic: VecDeque::new(),
            dynamic_size: 0,
            max_size: limit,
            limit,
            max_header_bytes,
            huffman: Huffman::new(),
        }
    }

    #[must_use]
    pub fn dynamic_table_size(&self) -> usize {
        self.dynamic_size
    }

    #[must_use]
    pub fn dynamic_table(&self) -> &VecDeque<(String, String)> {
        &self.dynamic
    }

    fn entry(&self, index: usize) -> Result<(String, String), HpackError> {
        let at = index.checked_sub(1).ok_or(HpackError("index 0"))?;
        if let Some((n, v)) = STATIC_TABLE.get(at) {
            return Ok(((*n).to_owned(), (*v).to_owned()));
        }
        self.dynamic
            .get(at - STATIC_TABLE.len())
            .cloned()
            .ok_or(HpackError("index out of range"))
    }

    fn evict(&mut self) {
        while self.dynamic_size > self.max_size {
            let Some((n, v)) = self.dynamic.pop_back() else {
                break;
            };
            self.dynamic_size -= entry_size(&n, &v);
        }
    }

    fn add(&mut self, name: String, value: String) {
        let size = entry_size(&name, &value);
        if size > self.max_size {
            self.dynamic.clear();
            self.dynamic_size = 0;
            return;
        }
        self.dynamic_size += size;
        self.dynamic.push_front((name, value));
        self.evict();
    }

    fn read_string(&self, buf: &[u8], pos: &mut usize) -> Result<String, HpackError> {
        let huffman = buf.get(*pos).ok_or(HpackError("truncated string"))? & 0x80 != 0;
        let len = decode_int(buf, pos, 7)?;
        let end = pos
            .checked_add(len)
            .filter(|e| *e <= buf.len())
            .ok_or(HpackError("truncated string"))?;
        let raw = &buf[*pos..end];
        *pos = end;
        Ok(if huffman {
            latin1(&self.huffman.decode(raw)?)
        } else {
            latin1(raw)
        })
    }

    fn name_and_value(
        &self,
        buf: &[u8],
        pos: &mut usize,
        prefix: u8,
    ) -> Result<(String, String), HpackError> {
        let idx = decode_int(buf, pos, prefix)?;
        let name = if idx == 0 {
            self.read_string(buf, pos)?
        } else {
            self.entry(idx)?.0
        };
        Ok((name, self.read_string(buf, pos)?))
    }

    /// One complete header block (HEADERS and CONTINUATION fragments joined). The dynamic table is
    /// connection state, so every block of the connection must pass through here, in order.
    pub fn decode(&mut self, buf: &[u8]) -> Result<Headers, HpackError> {
        let (mut out, mut pos, mut total) = (Headers::new(), 0usize, 0usize);
        while pos < buf.len() {
            let b = buf[pos];
            let field = if b & 0x80 != 0 {
                Some(self.entry(decode_int(buf, &mut pos, 7)?)?)
            } else if b & 0x40 != 0 {
                let (name, value) = self.name_and_value(buf, &mut pos, 6)?;
                self.add(name.clone(), value.clone());
                Some((name, value))
            } else if b & 0x20 != 0 {
                if !out.is_empty() {
                    return Err(HpackError("table size update after a header field"));
                }
                let size = decode_int(buf, &mut pos, 5)?;
                if size > self.limit {
                    return Err(HpackError("table size update exceeds the limit"));
                }
                self.max_size = size;
                self.evict();
                None
            } else {
                Some(self.name_and_value(buf, &mut pos, 4)?)
            };
            if let Some((name, value)) = field {
                total += name.len() + value.len();
                if total > self.max_header_bytes {
                    return Err(HpackError("header list too large"));
                }
                out.push((name, value));
            }
        }
        Ok(out)
    }
}
