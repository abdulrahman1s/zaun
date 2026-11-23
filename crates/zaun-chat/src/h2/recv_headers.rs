//! Response header blocks: HEADERS with CONTINUATION, decoded in order (RFC 9113 section 6.2, RFC 7541).
use std::io::{Read, Write};

use super::connection::Connection;
use super::error::H2Error;
use super::frames::{END_HEADERS, END_STREAM, FrameHeader, PRIORITY_FLAG};
use super::recv::unpadded;
use super::state::{Exchange, HeaderBlock};

impl<T: Read + Write> Connection<T> {
    pub(super) fn on_headers(
        &mut self,
        ex: &mut Exchange,
        h: FrameHeader,
        p: &[u8],
    ) -> Result<(), H2Error> {
        if h.stream == 0 {
            return Err(H2Error::Protocol("HEADERS on stream 0"));
        }
        let mut frag = unpadded(h.flags, p)?;
        if h.flags & PRIORITY_FLAG != 0 {
            frag = frag.get(5..).ok_or(H2Error::Protocol("bad HEADERS"))?;
        }
        let end_stream = h.flags & END_STREAM != 0;
        if h.flags & END_HEADERS != 0 {
            return self.header_block(ex, h.stream, frag, end_stream);
        }
        self.block = Some(HeaderBlock {
            stream: h.stream,
            bytes: frag.to_vec(),
            end_stream,
        });
        Ok(())
    }

    pub(super) fn on_continuation(
        &mut self,
        ex: &mut Exchange,
        h: FrameHeader,
        p: &[u8],
    ) -> Result<(), H2Error> {
        let Some(mut block) = self.block.take() else {
            return Err(H2Error::Protocol("unexpected CONTINUATION"));
        };
        block.bytes.extend_from_slice(p);
        if block.bytes.len() > self.opts.max_header_bytes {
            return Err(H2Error::Protocol("header block too large"));
        }
        if h.flags & END_HEADERS == 0 {
            self.block = Some(block);
            return Ok(());
        }
        self.header_block(ex, block.stream, &block.bytes, block.end_stream)
    }

    fn header_block(
        &mut self,
        ex: &mut Exchange,
        stream: u32,
        bytes: &[u8],
        end_stream: bool,
    ) -> Result<(), H2Error> {
        let list = self.hpack.decode(bytes)?; // always: the dynamic table is connection state
        if stream != ex.id {
            return Ok(());
        }
        if ex.headers.is_none() {
            let status = list
                .iter()
                .find(|(n, _)| n == ":status")
                .and_then(|(_, v)| v.parse::<u16>().ok());
            match status {
                None => return Err(H2Error::Protocol("response without :status")),
                Some(100..=199) => return Ok(()),
                Some(_) => ex.headers = Some(list),
            }
        } else {
            ex.trailers = list;
        }
        ex.ended |= end_stream;
        Ok(())
    }
}
