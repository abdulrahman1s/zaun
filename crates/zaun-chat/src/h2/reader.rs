//! Frame reader that keeps partial frames across read timeouts, so a timeout is only a chance to look at the cancel flag.
use std::io::{ErrorKind, Read};

use super::error::H2Error;
use super::frames::{FrameHeader, HEADER_LEN, parse_header};

#[derive(Default)]
pub(super) struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    fn take(&mut self, max_payload: usize) -> Result<Option<(FrameHeader, Vec<u8>)>, H2Error> {
        let Some(head) = self.buf.first_chunk::<HEADER_LEN>() else {
            return Ok(None);
        };
        let header = parse_header(head);
        if header.length > max_payload {
            return Err(H2Error::FrameSize(
                "frame larger than the announced maximum",
            ));
        }
        let end = HEADER_LEN + header.length;
        if self.buf.len() < end {
            return Ok(None);
        }
        let payload = self.buf[HEADER_LEN..end].to_vec();
        self.buf.drain(..end);
        Ok(Some((header, payload)))
    }

    /// The next whole frame, or `None` when the socket's read timeout fired first.
    pub(super) fn next_frame(
        &mut self,
        io: &mut impl Read,
        max_payload: usize,
    ) -> Result<Option<(FrameHeader, Vec<u8>)>, H2Error> {
        loop {
            if let Some(frame) = self.take(max_payload)? {
                return Ok(Some(frame));
            }
            let mut chunk = [0u8; 32768];
            match io.read(&mut chunk) {
                Ok(0) => return Err(H2Error::Closed),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Ok(None);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
}
