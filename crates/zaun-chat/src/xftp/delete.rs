//! Deleting an uploaded file from the servers with its sender description (Agent.hs `deleteSndFilesRemote`).
use std::sync::atomic::AtomicBool;

use crate::crypto::PrivateKey;
use crate::h2::Progress;
use crate::smp::{Clock, SystemClock};

use super::client::{ClientConfig, Ctl};
use super::description::{FileDescription, Party};
use super::error::XftpError;
use super::error_type::XftpErrorType;
use super::pool::Pool;

const ATTEMPTS: usize = 3;

/// Sends `FDEL` for every chunk. A chunk the server no longer knows counts as deleted. Every chunk is tried;
/// the first other failure is returned at the end.
pub fn delete_file(sender: &FileDescription, cancel: &AtomicBool) -> Result<(), XftpError> {
    if sender.party != Party::Sender {
        return Err(XftpError::Description(
            "a sender description is needed to delete".into(),
        ));
    }
    let clock = SystemClock::new();
    let mut pool = Pool::new(ClientConfig::default());
    let mut first_error = None;
    for chunk in &sender.chunks {
        let Some(r) = chunk.replicas.first() else {
            continue;
        };
        let key = PrivateKey::from_der(&r.replica_key)
            .map_err(|_| XftpError::Description("bad replica key".into()))?;
        let done = pool.run(&r.server, cancel, ATTEMPTS, |c| {
            let mut ignore = |_: Progress| {};
            let mut ctl = Ctl {
                cancel,
                clock: &clock as &dyn Clock,
                on_progress: &mut ignore,
            };
            c.delete_chunk(&key, &r.replica_id, &mut ctl)
        });
        match done {
            Ok(()) | Err(XftpError::Server(XftpErrorType::Auth | XftpErrorType::NoFile)) => {}
            Err(XftpError::Cancelled) => return Err(XftpError::Cancelled),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}
