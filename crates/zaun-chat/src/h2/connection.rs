//! A synchronous HTTP/2 client connection with one request in flight (RFC 9113); the part XFTP needs.
use std::io::{Read, Write};

use super::error::{H2Error, Stop};
use super::frames::{
    CANCEL, CLIENT_PREFACE, DEFAULT_MAX_FRAME, DEFAULT_WINDOW, SETTINGS_ENABLE_PUSH,
    SETTINGS_HEADER_TABLE_SIZE, SETTINGS_INITIAL_WINDOW_SIZE, SETTINGS_MAX_FRAME_SIZE,
    encode_goaway, encode_rst_stream, encode_settings, encode_window_update,
};
use super::hpack_decode::HpackDecoder;
use super::reader::FrameReader;
use super::response::{Progress, Request, Response};
use super::state::{Exchange, HeaderBlock, Options};

pub struct Connection<T: Read + Write> {
    pub(super) io: T,
    pub(super) reader: FrameReader,
    pub(super) hpack: HpackDecoder,
    pub(super) opts: Options,
    pub(super) next_stream: u32,
    pub(super) peer_max_frame: usize,
    pub(super) peer_initial_window: i64,
    pub(super) conn_send: i64,
    pub(super) conn_recv_consumed: u32,
    pub(super) block: Option<HeaderBlock>,
    pub(super) going_away: bool,
    dead: bool,
}

impl<T: Read + Write> Connection<T> {
    /// Sends the client preface and SETTINGS; the server's SETTINGS are read with the first response.
    pub fn open(mut io: T, opts: Options) -> Result<Self, H2Error> {
        let settings = [
            (SETTINGS_HEADER_TABLE_SIZE, 4096),
            (SETTINGS_ENABLE_PUSH, 0),
            (SETTINGS_INITIAL_WINDOW_SIZE, opts.stream_window),
            (SETTINGS_MAX_FRAME_SIZE, DEFAULT_MAX_FRAME as u32),
        ];
        let mut hello = [CLIENT_PREFACE, &encode_settings(&settings)].concat();
        let extra = i64::from(opts.connection_window) - DEFAULT_WINDOW;
        if extra > 0 {
            hello.extend(encode_window_update(0, extra as u32));
        }
        io.write_all(&hello)?;
        io.flush()?;
        Ok(Self {
            io,
            reader: FrameReader::default(),
            hpack: HpackDecoder::new(4096, opts.max_header_bytes),
            opts,
            next_stream: 1,
            peer_max_frame: DEFAULT_MAX_FRAME,
            peer_initial_window: DEFAULT_WINDOW,
            conn_send: DEFAULT_WINDOW,
            conn_recv_consumed: 0,
            block: None,
            going_away: false,
            dead: false,
        })
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> Result<(), H2Error> {
        self.io.write_all(bytes)?;
        self.io.flush()?;
        Ok(())
    }

    /// The transport, for the caller's timeout and shutdown handling.
    pub fn io_mut(&mut self) -> &mut T {
        &mut self.io
    }

    #[must_use]
    pub fn is_usable(&self) -> bool {
        !self.dead && !self.going_away
    }

    /// Sends `req` and reads the whole response, at most `max_body` bytes of body. `watch` runs after every
    /// frame and on every read timeout of the transport; an `Err` from it abandons the request.
    /// Any failure makes the connection unusable.
    pub fn request(
        &mut self,
        req: &Request<'_>,
        max_body: usize,
        watch: &mut dyn FnMut(Progress) -> Result<(), Stop>,
    ) -> Result<Response, H2Error> {
        if !self.is_usable() {
            return Err(H2Error::Closed);
        }
        let result = self.exchange(req, max_body, watch);
        if result.is_err() {
            self.dead = true;
        }
        result
    }

    fn exchange(
        &mut self,
        req: &Request<'_>,
        max_body: usize,
        watch: &mut dyn FnMut(Progress) -> Result<(), Stop>,
    ) -> Result<Response, H2Error> {
        if self.next_stream > 0x7fff_fffd {
            return Err(H2Error::Protocol("stream ids exhausted"));
        }
        let mut ex = Exchange {
            id: self.next_stream,
            send_window: self.peer_initial_window,
            recv_window: i64::from(self.opts.stream_window),
            headers: None,
            trailers: Vec::new(),
            body: Vec::new(),
            max_body,
            sent: 0,
            ended: false,
        };
        self.next_stream += 2;
        self.send_headers(&ex, req)?;
        loop {
            let progressed = self.send_batch(&mut ex, req.body)?;
            let progress = Progress {
                sent: ex.sent,
                received: ex.body.len(),
            };
            watch(progress).map_err(H2Error::Stopped)?;
            if ex.ended {
                break;
            }
            if progressed {
                continue;
            }
            if let Some((header, payload)) =
                self.reader.next_frame(&mut self.io, DEFAULT_MAX_FRAME)?
            {
                self.on_frame(&mut ex, header, payload)?;
            }
        }
        if ex.sent < req.body.len() {
            self.write(&encode_rst_stream(ex.id, CANCEL))?;
        }
        let status = ex
            .headers
            .as_deref()
            .and_then(|h| h.iter().find(|(n, _)| n == ":status"))
            .and_then(|(_, v)| v.parse().ok());
        let (Some(status), Some(headers)) = (status, ex.headers) else {
            return Err(H2Error::Protocol("response without :status"));
        };
        Ok(Response {
            status,
            headers,
            trailers: ex.trailers,
            body: ex.body,
        })
    }

    /// Graceful close: GOAWAY; the caller shuts the transport down.
    pub fn close(&mut self) {
        if !self.dead {
            let _ = self.write(&encode_goaway(0, 0)); // the peer may be gone already
        }
        self.dead = true;
    }
}
