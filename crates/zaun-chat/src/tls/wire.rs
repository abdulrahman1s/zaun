//! Reading and writing the TLS presentation language (RFC 8446 section 3): vectors and u24s.
use super::error::TlsError;

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub(super) fn take(&mut self, n: usize) -> Result<&'a [u8], TlsError> {
        let (head, tail) = self
            .bytes
            .split_at_checked(n)
            .ok_or(TlsError::Decode("truncated"))?;
        self.bytes = tail;
        Ok(head)
    }

    pub(super) fn u8(&mut self) -> Result<u8, TlsError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, TlsError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub(super) fn vec8(&mut self) -> Result<&'a [u8], TlsError> {
        let n = usize::from(self.u8()?);
        self.take(n)
    }

    pub(super) fn vec16(&mut self) -> Result<&'a [u8], TlsError> {
        let n = usize::from(self.u16()?);
        self.take(n)
    }

    pub(super) fn vec24(&mut self) -> Result<&'a [u8], TlsError> {
        let b = self.take(3)?;
        self.take(u24_value(b))
    }

    pub(super) fn finish(self) -> Result<(), TlsError> {
        if self.bytes.is_empty() {
            Ok(())
        } else {
            Err(TlsError::Decode("trailing bytes"))
        }
    }
}

pub(super) fn u24_value(b: &[u8]) -> usize {
    b.iter().take(3).fold(0, |a, x| a << 8 | usize::from(*x))
}

/// Removes and returns the first complete handshake message (header included) from `pending`.
pub(super) fn take_handshake_message(pending: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    let len = u24_value(pending.get(1..4)?);
    if pending.len() < 4 + len {
        return None;
    }
    let raw: Vec<u8> = pending.drain(..4 + len).collect();
    Some((raw[0], raw))
}

/// Extensions of a hello or EncryptedExtensions body as `(type, data)` pairs.
pub(super) fn extensions(block: &[u8]) -> Result<Vec<(u16, &[u8])>, TlsError> {
    let mut r = Reader::new(block);
    let mut out = Vec::new();
    while !r.is_empty() {
        out.push((r.u16()?, r.vec16()?));
    }
    Ok(out)
}

pub(super) fn put_u16(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&(n as u16).to_be_bytes());
}

pub(super) fn put_u24(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&(n as u32).to_be_bytes()[1..]);
}

/// A handshake message: type, u24 length, body.
pub(super) fn handshake_message(ty: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![ty];
    put_u24(&mut out, body.len());
    out.extend_from_slice(body);
    out
}

pub(super) fn extension(ty: u16, data: &[u8]) -> Vec<u8> {
    let mut out = ty.to_be_bytes().to_vec();
    put_u16(&mut out, data.len());
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_rejects_a_vector_longer_than_its_input() {
        let mut r = Reader::new(&[0, 5, 1, 2]);
        assert!(matches!(r.vec16(), Err(TlsError::Decode(_))));
    }

    #[test]
    fn reader_finish_rejects_trailing_bytes() {
        let mut r = Reader::new(&[1, 2]);
        r.u8().unwrap();
        assert!(matches!(r.finish(), Err(TlsError::Decode(_))));
    }

    #[test]
    fn u24_vectors_round_trip_through_handshake_message() {
        let m = handshake_message(11, &[7; 300]);
        let mut r = Reader::new(&m[1..]);
        assert_eq!(r.vec24().unwrap(), &[7; 300][..]);
        r.finish().unwrap();
    }
}
