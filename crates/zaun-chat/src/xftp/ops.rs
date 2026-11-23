//! The chunk operations of an XFTP client (Client.hs `createXFTPChunk` … `pingXFTP`).
use std::io::{Read, Write};

use crate::crypto::{PrivateKey, PublicKey};

use super::client::{Ctl, XftpClient};
use super::commands::{Command, FileInfo};
use super::error::XftpError;

/// A chunk the server created: the sender's id and one recipient id per key given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChunk {
    pub sender_id: Vec<u8>,
    pub recipient_ids: Vec<Vec<u8>>,
    /// Epoch seconds the server keeps the chunk until, from v4.
    pub expires: Option<i64>,
}

pub(super) fn unexpected() -> XftpError {
    XftpError::Unexpected("response to the command")
}

impl<T: Read + Write> XftpClient<T> {
    pub fn create_chunk(
        &mut self,
        sender_key: &PrivateKey,
        info: &FileInfo,
        rcv_keys: &[PublicKey],
        basic_auth: Option<&str>,
        storage_hours: Option<u32>,
        ctl: &mut Ctl<'_>,
    ) -> Result<NewChunk, XftpError> {
        let cmd = Command::New {
            info: info.clone(),
            rcv_keys: rcv_keys.to_vec(),
            basic_auth: basic_auth.map(str::to_owned),
            storage_hours,
        };
        let (sender_id, recipient_ids, expires) = self
            .call(Some(sender_key), &[], &cmd, &[], 0, ctl)?
            .0
            .into_snd_ids()
            .filter(|(_, ids, _)| ids.len() == rcv_keys.len())
            .ok_or_else(unexpected)?;
        Ok(NewChunk {
            sender_id,
            recipient_ids,
            expires,
        })
    }

    pub fn add_recipients(
        &mut self,
        sender_key: &PrivateKey,
        file_id: &[u8],
        rcv_keys: &[PublicKey],
        ctl: &mut Ctl<'_>,
    ) -> Result<Vec<Vec<u8>>, XftpError> {
        let cmd = Command::AddRecipients {
            rcv_keys: rcv_keys.to_vec(),
        };
        self.call(Some(sender_key), file_id, &cmd, &[], 0, ctl)?
            .0
            .into_rcv_ids()
            .filter(|ids| ids.len() == rcv_keys.len())
            .ok_or_else(unexpected)
    }

    pub fn upload_chunk(
        &mut self,
        sender_key: &PrivateKey,
        file_id: &[u8],
        chunk: &[u8],
        ctl: &mut Ctl<'_>,
    ) -> Result<(), XftpError> {
        let answer = self
            .call(Some(sender_key), file_id, &Command::Put, chunk, 0, ctl)?
            .0;
        if answer.is_ok() {
            Ok(())
        } else {
            Err(unexpected())
        }
    }

    pub fn delete_chunk(
        &mut self,
        sender_key: &PrivateKey,
        file_id: &[u8],
        ctl: &mut Ctl<'_>,
    ) -> Result<(), XftpError> {
        let answer = self
            .call(Some(sender_key), file_id, &Command::Delete, &[], 0, ctl)?
            .0;
        if answer.is_ok() {
            Ok(())
        } else {
            Err(unexpected())
        }
    }

    /// `FACK`: the recipient says it has the chunk. The agent never sends it; kept for the protocol's sake.
    pub fn ack_chunk(
        &mut self,
        recipient_key: &PrivateKey,
        file_id: &[u8],
        ctl: &mut Ctl<'_>,
    ) -> Result<(), XftpError> {
        let answer = self
            .call(Some(recipient_key), file_id, &Command::Ack, &[], 0, ctl)?
            .0;
        if answer.is_ok() {
            Ok(())
        } else {
            Err(unexpected())
        }
    }

    pub fn ping(&mut self, ctl: &mut Ctl<'_>) -> Result<(), XftpError> {
        let answer = self.call(None, &[], &Command::Ping, &[], 0, ctl)?.0;
        if answer.is_pong() {
            Ok(())
        } else {
            Err(unexpected())
        }
    }
}
