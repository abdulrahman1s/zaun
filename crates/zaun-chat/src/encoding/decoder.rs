//! A cursor over a byte slice, the Rust side of the attoparsec parser (Encoding.hs, encoding.ts `Decoder`).
use super::error::DecodeError;

#[derive(Debug, Clone)]
pub struct Decoder<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Eof)?;
        let slice = self.buf.get(self.pos..end).ok_or(DecodeError::Eof)?;
        self.pos = end;
        Ok(slice)
    }

    pub fn take_array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub fn take_all(&mut self) -> &'a [u8] {
        let rest = self.buf.get(self.pos..).unwrap_or_default();
        self.pos = self.buf.len();
        rest
    }

    pub fn byte(&mut self) -> Result<u8, DecodeError> {
        let [b] = self.take_array()?;
        Ok(b)
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    #[must_use]
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// `parseAll`: fails when input is left over.
    pub fn finish(&self) -> Result<(), DecodeError> {
        match self.remaining() {
            0 => Ok(()),
            n => Err(DecodeError::Trailing(n)),
        }
    }
}
