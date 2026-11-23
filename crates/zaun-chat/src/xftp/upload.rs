//! Uploading a file: encrypt, cut into chunks, put them on the servers, return the descriptions (Agent.hs send workers).
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::smp::{Clock, SystemClock};

use super::address::XftpServer;
use super::chunks::ChunkSpec;
use super::client::ClientConfig;
use super::description::FileDescription;
use super::error::XftpError;
use super::pool::Pool;
use super::progress::{CancelFlag, ProgressFn};
use super::upload_chunk::{Job, SentChunk, send_chunk};
use super::upload_descr::describe;
use super::upload_prepare::prepare;

pub struct UploadParams<'a> {
    pub source: &'a Path,
    /// The name the recipient sees, in the encrypted header.
    pub file_name: &'a str,
    /// Where the encrypted copy lives while it is sent; removed at the end.
    pub work_dir: &'a Path,
    pub servers: &'a [XftpServer],
    /// How many recipients each chunk is made for (the chat uses 4 for a direct file).
    pub recipients: usize,
    pub storage_hours: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uploaded {
    /// Keeps the replica keys that delete the file; keep it to call `delete_file` later.
    pub sender: FileDescription,
    /// One description per recipient.
    pub recipients: Vec<FileDescription>,
    pub encrypted_size: u64,
    /// The earliest time (epoch seconds) a server may drop a chunk, when the servers say.
    pub expires: Option<i64>,
}

fn read_chunk(file: &mut File, spec: ChunkSpec) -> Result<Vec<u8>, XftpError> {
    let mut bytes =
        vec![0u8; usize::try_from(spec.size).map_err(|_| XftpError::File("chunk size".into()))?];
    file.seek(SeekFrom::Start(spec.offset))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Blocks until every chunk is up. `progress(done, total)` counts encrypted bytes sent; `cancel` aborts at the next network wait.
pub fn upload(
    params: &UploadParams<'_>,
    cancel: &AtomicBool,
    progress: &mut ProgressFn<'_>,
) -> Result<Uploaded, XftpError> {
    if params.servers.is_empty() {
        return Err(XftpError::NoServers);
    }
    let recipients = params.recipients.max(1);
    let prepared = prepare(
        params.source,
        params.file_name,
        params.work_dir,
        CancelFlag(cancel),
    )?;
    let clock = SystemClock::new();
    let job = Job {
        servers: params.servers,
        recipients,
        storage_hours: params.storage_hours,
        cancel,
        clock: &clock as &dyn Clock,
    };
    let mut pool = Pool::new(ClientConfig::default());
    let mut file = File::open(&prepared.path)?;
    let (mut sent, mut done) = (Vec::<SentChunk>::new(), 0u64);
    progress(0, prepared.enc_size);
    for spec in &prepared.specs {
        let bytes = read_chunk(&mut file, *spec)?;
        let total = prepared.enc_size;
        let chunk = send_chunk(&job, &mut pool, *spec, &bytes, &mut |n| {
            progress(done + n as u64, total)
        })?;
        done += u64::from(spec.size);
        progress(done, total);
        sent.push(chunk);
    }
    let expires = sent
        .iter()
        .map(|c| c.expires)
        .collect::<Option<Vec<_>>>()
        .and_then(|v| v.into_iter().min());
    let (sender, recipients) = describe(&prepared, &sent, recipients)
        .ok_or(XftpError::Unexpected("chunk without recipient"))?;
    Ok(Uploaded {
        sender,
        recipients,
        encrypted_size: prepared.enc_size,
        expires,
    })
}
