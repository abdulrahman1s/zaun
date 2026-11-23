//! HPACK Huffman strings (RFC 7541 section 5.2): encoder and a bitwise trie decoder.
use super::hpack_int::HpackError;
use super::huffman_table::{CODES, LENGTHS};

const EOS: u16 = 256;
const NONE: u16 = u16::MAX;

#[must_use]
pub fn huffman_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let (mut acc, mut bits) = (0u64, 0u32);
    for &b in data {
        let len = u32::from(LENGTHS[usize::from(b)]);
        acc = acc << len | u64::from(CODES[usize::from(b)]);
        bits += len;
        while bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push((acc << (8 - bits)) as u8 | ((1u8 << (8 - bits)) - 1));
    }
    out
}

/// The decoding trie; build it once per connection.
pub struct Huffman {
    children: Vec<[u16; 2]>,
    symbol: Vec<u16>,
}

impl Huffman {
    #[must_use]
    pub fn new() -> Self {
        let mut trie = Self {
            children: vec![[NONE; 2]],
            symbol: vec![NONE],
        };
        for sym in 0..=EOS {
            let (len, code) = (LENGTHS[usize::from(sym)], CODES[usize::from(sym)]);
            let mut node = 0usize;
            for k in (0..len).rev() {
                let bit = usize::try_from(code >> k & 1).unwrap_or(0);
                if trie.children[node][bit] == NONE {
                    trie.children.push([NONE; 2]);
                    trie.symbol.push(NONE);
                    trie.children[node][bit] = (trie.children.len() - 1) as u16;
                }
                node = usize::from(trie.children[node][bit]);
            }
            trie.symbol[node] = sym;
        }
        trie
    }

    pub fn decode(&self, data: &[u8]) -> Result<Vec<u8>, HpackError> {
        let mut out = Vec::with_capacity(data.len() * 2);
        let (mut node, mut pending, mut ones) = (0usize, 0u32, true);
        for &byte in data {
            for k in (0..8).rev() {
                let bit = usize::from(byte >> k & 1);
                let next = self.children[node][bit];
                if next == NONE {
                    return Err(HpackError("invalid huffman code"));
                }
                node = usize::from(next);
                pending += 1;
                ones &= bit == 1;
                match self.symbol[node] {
                    NONE => {}
                    EOS => return Err(HpackError("EOS in huffman string")),
                    sym => {
                        out.push(sym as u8);
                        (node, pending, ones) = (0, 0, true);
                    }
                }
            }
        }
        if pending > 7 || !ones {
            return Err(HpackError("invalid huffman padding"));
        }
        Ok(out)
    }
}

impl Default for Huffman {
    fn default() -> Self {
        Self::new()
    }
}
