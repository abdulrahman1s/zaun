//! Minimal reader and writer for the SMP binary encoding used by `=`, `/` and `>` elements.
// TODO(encoding): use crate::encoding once it lands
use super::error::{ChatError, Result, parse_err};

pub(crate) struct Reader<'a>(pub &'a [u8]);

impl<'a> Reader<'a> {
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let (head, tail) = self
            .0
            .split_at_checked(n)
            .ok_or_else(|| ChatError::Parse("short binary input".into()))?;
        self.0 = tail;
        Ok(head)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn i64(&mut self) -> Result<i64> {
        let b: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| ChatError::Parse("short binary input".into()))?;
        Ok(i64::from_be_bytes(b))
    }

    /// A `ByteString`: one length byte, then the bytes.
    pub fn short_bytes(&mut self) -> Result<&'a [u8]> {
        let n = usize::from(self.u8()?);
        self.take(n)
    }

    /// A `Large`: two length bytes (big endian), then the bytes.
    pub fn large(&mut self) -> Result<&'a [u8]> {
        let n = usize::from(self.u16()?);
        self.take(n)
    }

    pub fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.0)
    }

    pub fn finish(&self) -> Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            parse_err("unexpected trailing bytes")
        }
    }
}

pub(crate) fn put_short(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    out.push(
        u8::try_from(bytes.len())
            .map_err(|_| ChatError::Limit("short binary field over 255 bytes"))?,
    );
    out.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn put_large(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let n = u16::try_from(bytes.len())
        .map_err(|_| ChatError::Limit("large binary field over 65535 bytes"))?;
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}
