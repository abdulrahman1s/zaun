//! Handling the server's control frames while a request is in flight (RFC 9113 section 6).
use std::io::{Read, Write};

use super::connection::Connection;
use super::error::H2Error;
use super::frames::{
    ACK, CONTINUATION, DATA, FrameHeader, GOAWAY, HEADERS, MAX_WINDOW, NO_ERROR, PADDED, PING,
    PRIORITY, PUSH_PROMISE, RST_STREAM, SETTINGS, SETTINGS_INITIAL_WINDOW_SIZE,
    SETTINGS_MAX_FRAME_SIZE, WINDOW_UPDATE, decode_settings, encode_frame, encode_ping,
};
use super::state::Exchange;

pub(super) fn be32(p: &[u8]) -> u32 {
    p.first_chunk::<4>().map_or(0, |b| u32::from_be_bytes(*b))
}

/// The content of a DATA or HEADERS payload without padding.
pub(super) fn unpadded(flags: u8, p: &[u8]) -> Result<&[u8], H2Error> {
    if flags & PADDED == 0 {
        return Ok(p);
    }
    let pad = usize::from(*p.first().ok_or(H2Error::Protocol("bad padding"))?);
    p.get(1..p.len().saturating_sub(pad))
        .filter(|_| pad < p.len())
        .ok_or(H2Error::Protocol("bad padding"))
}

impl<T: Read + Write> Connection<T> {
    pub(super) fn on_frame(
        &mut self,
        ex: &mut Exchange,
        h: FrameHeader,
        p: Vec<u8>,
    ) -> Result<(), H2Error> {
        if let Some(b) = &self.block
            && (h.kind != CONTINUATION || h.stream != b.stream)
        {
            return Err(H2Error::Protocol("expected CONTINUATION"));
        }
        match h.kind {
            DATA => self.on_data(ex, h, &p),
            HEADERS => self.on_headers(ex, h, &p),
            CONTINUATION => self.on_continuation(ex, h, &p),
            PRIORITY => Ok(()),
            RST_STREAM if p.len() != 4 => Err(H2Error::FrameSize("RST_STREAM")),
            RST_STREAM if h.stream == ex.id => Err(H2Error::Reset(be32(&p))),
            SETTINGS => self.on_settings(ex, h, &p),
            PUSH_PROMISE => Err(H2Error::Protocol("PUSH_PROMISE")),
            PING => self.on_ping(h, &p),
            GOAWAY => self.on_goaway(ex, &p),
            WINDOW_UPDATE => self.on_window_update(ex, h, &p),
            _ => Ok(()),
        }
    }

    pub(super) fn on_settings(
        &mut self,
        ex: &mut Exchange,
        h: FrameHeader,
        p: &[u8],
    ) -> Result<(), H2Error> {
        if h.stream != 0 {
            return Err(H2Error::Protocol("SETTINGS on a stream"));
        }
        if h.flags & ACK != 0 {
            return Ok(());
        }
        for (id, v) in decode_settings(p).ok_or(H2Error::FrameSize("SETTINGS"))? {
            match id {
                SETTINGS_INITIAL_WINDOW_SIZE => {
                    let v = i64::from(v);
                    if v > MAX_WINDOW {
                        return Err(H2Error::FlowControl("INITIAL_WINDOW_SIZE"));
                    }
                    ex.send_window += v - self.peer_initial_window;
                    self.peer_initial_window = v;
                }
                SETTINGS_MAX_FRAME_SIZE => {
                    if !(16384..=16_777_215).contains(&v) {
                        return Err(H2Error::Protocol("MAX_FRAME_SIZE"));
                    }
                    self.peer_max_frame = v as usize;
                }
                _ => {} // we never index request headers, so the peer's table size does not matter
            }
        }
        self.write(&encode_frame(SETTINGS, ACK, 0, &[]))
    }

    pub(super) fn on_ping(&mut self, h: FrameHeader, p: &[u8]) -> Result<(), H2Error> {
        let data: [u8; 8] = p.try_into().map_err(|_| H2Error::FrameSize("PING"))?;
        if h.stream != 0 {
            return Err(H2Error::Protocol("PING on a stream"));
        }
        if h.flags & ACK == 0 {
            self.write(&encode_ping(data, true))
        } else {
            Ok(())
        }
    }

    pub(super) fn on_goaway(&mut self, ex: &Exchange, p: &[u8]) -> Result<(), H2Error> {
        if p.len() < 8 {
            return Err(H2Error::FrameSize("GOAWAY"));
        }
        let (last, code) = (be32(p) & 0x7fff_ffff, be32(&p[4..]));
        self.going_away = true;
        if ex.id > last || code != NO_ERROR {
            return Err(H2Error::GoAway(code));
        }
        Ok(())
    }

    pub(super) fn on_window_update(
        &mut self,
        ex: &mut Exchange,
        h: FrameHeader,
        p: &[u8],
    ) -> Result<(), H2Error> {
        if p.len() != 4 {
            return Err(H2Error::FrameSize("WINDOW_UPDATE"));
        }
        let inc = i64::from(be32(p) & 0x7fff_ffff);
        if inc == 0 {
            return Err(H2Error::Protocol("zero WINDOW_UPDATE"));
        }
        let window = match h.stream {
            0 => &mut self.conn_send,
            id if id == ex.id => &mut ex.send_window,
            _ => return Ok(()),
        };
        *window += inc;
        if *window > MAX_WINDOW {
            return Err(H2Error::FlowControl("window overflow"));
        }
        Ok(())
    }
}
