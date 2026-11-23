//! An XFTP connection: one command in flight over HTTP/2, each a padded block (Client.hs `sendXFTPCommand`).
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::crypto::PrivateKey;
use crate::h2::{Connection, Progress, Request, Stop};
use crate::smp::Clock;

use super::commands::Command;
use super::error::XftpError;
use super::responses::Response;
use super::transmission::{decode_transmission, encode_transmission};
use super::version::BLOCK_SIZE;

#[derive(Debug, Clone, Copy)]
pub struct ClientConfig {
    pub connect_timeout: Duration,
    /// How long one command may take besides moving its bytes.
    pub command_timeout: Duration,
    /// Extra time per KiB of chunk, so a slow link can finish a 4 MiB chunk.
    pub per_kib: Duration,
    /// How often a waiting read wakes up to look at the cancel flag.
    pub poll: Duration,
    /// How long one write may stall; a timed-out write ends the connection, so this is generous.
    pub write_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(20),
            command_timeout: Duration::from_secs(30),
            per_kib: Duration::from_millis(25),
            poll: Duration::from_millis(250),
            write_timeout: Duration::from_secs(30),
        }
    }
}

/// What a running command needs from its caller.
pub struct Ctl<'a> {
    pub cancel: &'a AtomicBool,
    pub clock: &'a dyn Clock,
    /// Body bytes moved by the current command.
    pub on_progress: &'a mut dyn FnMut(Progress),
}

pub struct XftpClient<T: Read + Write> {
    pub(super) conn: Connection<T>,
    pub(super) authority: String,
    pub(super) session_id: Vec<u8>,
    pub(super) version: u16,
    pub(super) cfg: ClientConfig,
}

impl<T: Read + Write> XftpClient<T> {
    #[must_use]
    pub fn version(&self) -> u16 {
        self.version
    }

    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.conn.is_usable()
    }

    /// Sends `cmd` and, after it, `file` bytes; returns the answer and the bytes after the response block.
    /// `max_file` bounds those. A server `ERR` is an `Err`.
    pub(super) fn call(
        &mut self,
        key: Option<&PrivateKey>,
        entity: &[u8],
        cmd: &Command,
        file: &[u8],
        max_file: usize,
        ctl: &mut Ctl<'_>,
    ) -> Result<(Response, Vec<u8>), XftpError> {
        let block = encode_transmission(
            &self.session_id,
            entity,
            &cmd.encode(self.version)?,
            key.filter(|_| cmd.is_authorized()),
        )?;
        let body = [block.as_slice(), file].concat();
        let kib = u32::try_from((file.len() + max_file) / 1024).unwrap_or(u32::MAX);
        let deadline = ctl.clock.now_ms()
            + u64::try_from((self.cfg.command_timeout + self.cfg.per_kib * kib).as_millis())
                .unwrap_or(u64::MAX);
        let (cancel, clock) = (ctl.cancel, ctl.clock);
        let on_progress = &mut *ctl.on_progress;
        let mut watch = |p: Progress| -> Result<(), Stop> {
            on_progress(p);
            if cancel.load(Ordering::Relaxed) {
                return Err(Stop::Cancelled);
            }
            if clock.now_ms() > deadline {
                Err(Stop::TimedOut)
            } else {
                Ok(())
            }
        };
        let req = Request {
            method: "POST",
            path: "/",
            authority: &self.authority,
            headers: &[],
            body: &body,
        };
        let mut resp = self.conn.request(&req, BLOCK_SIZE + max_file, &mut watch)?;
        if resp.status != 200 {
            return Err(XftpError::Status(resp.status));
        }
        if resp.body.len() < BLOCK_SIZE {
            return Err(XftpError::Unexpected("response shorter than a block"));
        }
        let rest = resp.body.split_off(BLOCK_SIZE);
        let (_entity, command) = decode_transmission(&self.session_id, &resp.body)?;
        match Response::decode(self.version, &command)? {
            Response::Err(e) => Err(XftpError::Server(e)),
            r @ (Response::SndIds { .. }
            | Response::RcvIds { .. }
            | Response::File { .. }
            | Response::Ok
            | Response::Pong) => Ok((r, rest)),
        }
    }
}
