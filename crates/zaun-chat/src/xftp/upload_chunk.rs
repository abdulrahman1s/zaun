//! Sending one chunk: create it on a server, add the other recipients, upload its bytes (Agent.hs `createChunk`, `uploadFileChunk`).
use std::sync::atomic::AtomicBool;

use crate::crypto::{Alg, KeyPair, PrivateKey, PublicKey, random_array, sha256};
use crate::h2::Progress;
use crate::smp::Clock;

use super::address::XftpServer;
use super::chunks::ChunkSpec;
use super::client::Ctl;
use super::commands::FileInfo;
use super::error::XftpError;
use super::pool::Pool;
use super::version::BLOCK_SIZE;

/// Recipient keys one FNEW or FADD carries (Agent's `xftpMaxRecipientsPerRequest`).
const MAX_PER_REQUEST: usize = 200;
const ATTEMPTS: usize = 3;

pub(super) struct SentChunk {
    pub(super) spec: ChunkSpec,
    pub(super) digest: Vec<u8>,
    pub(super) server: XftpServer,
    pub(super) sender_id: Vec<u8>,
    pub(super) sender_key: Vec<u8>,
    /// Id and PKCS#8 key per recipient.
    pub(super) recipients: Vec<(Vec<u8>, Vec<u8>)>,
    pub(super) expires: Option<i64>,
}

pub(super) struct Job<'a> {
    pub(super) servers: &'a [XftpServer],
    pub(super) recipients: usize,
    pub(super) storage_hours: Option<u32>,
    pub(super) cancel: &'a AtomicBool,
    pub(super) clock: &'a dyn Clock,
}

fn keys(n: usize) -> Result<Vec<KeyPair>, XftpError> {
    Ok((0..n)
        .map(|_| KeyPair::generate(Alg::Ed25519))
        .collect::<Result<_, _>>()?)
}

fn publics(pairs: &[KeyPair]) -> Vec<PublicKey> {
    pairs.iter().map(|p| p.public.clone()).collect()
}

/// A chunk the first server that took it created.
struct Created {
    server: XftpServer,
    recipient_ids: Vec<Vec<u8>>,
    sender_id: Vec<u8>,
    expires: Option<i64>,
}

/// Creates the chunk on the first server that takes it, starting from a random one.
fn create(
    job: &Job<'_>,
    pool: &mut Pool,
    info: &FileInfo,
    sender: &PrivateKey,
    rcv: &[KeyPair],
) -> Result<Created, XftpError> {
    let first = usize::from(random_array::<1>()?[0]);
    let mut last = XftpError::NoServers;
    let mut ignore = |_: Progress| {};
    for i in 0..job.servers.len() {
        let server = &job.servers[(first + i) % job.servers.len()];
        let head = &rcv[..rcv.len().min(MAX_PER_REQUEST)];
        let made = pool.run(server, job.cancel, ATTEMPTS, |c| {
            let mut ctl = Ctl {
                cancel: job.cancel,
                clock: job.clock,
                on_progress: &mut ignore,
            };
            c.create_chunk(
                sender,
                info,
                &publics(head),
                server.basic_auth.as_deref(),
                job.storage_hours,
                &mut ctl,
            )
        });
        match made {
            Ok(c) => {
                return Ok(Created {
                    server: server.clone(),
                    recipient_ids: c.recipient_ids,
                    sender_id: c.sender_id,
                    expires: c.expires,
                });
            }
            Err(e) if e.is_temporary() => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

pub(super) fn send_chunk(
    job: &Job<'_>,
    pool: &mut Pool,
    spec: ChunkSpec,
    bytes: &[u8],
    progress: &mut dyn FnMut(usize),
) -> Result<SentChunk, XftpError> {
    let digest = sha256(bytes).to_vec();
    let sender = KeyPair::generate(Alg::Ed25519)?;
    let rcv = keys(job.recipients.max(1))?;
    let info = FileInfo {
        snd_key: sender.public.clone(),
        size: spec.size,
        digest: digest.clone(),
    };
    let Created {
        server,
        recipient_ids: mut ids,
        sender_id,
        expires,
    } = create(job, pool, &info, &sender.private, &rcv)?;
    for batch in rcv[ids.len()..].chunks(MAX_PER_REQUEST) {
        let more = pool.run(&server, job.cancel, ATTEMPTS, |c| {
            let mut ignore = |_: Progress| {};
            let mut ctl = Ctl {
                cancel: job.cancel,
                clock: job.clock,
                on_progress: &mut ignore,
            };
            c.add_recipients(&sender.private, &sender_id, &publics(batch), &mut ctl)
        })?;
        ids.extend(more);
    }
    pool.run(&server, job.cancel, ATTEMPTS, |c| {
        let mut on_progress = |p: Progress| progress(p.sent.saturating_sub(BLOCK_SIZE));
        let mut ctl = Ctl {
            cancel: job.cancel,
            clock: job.clock,
            on_progress: &mut on_progress,
        };
        c.upload_chunk(&sender.private, &sender_id, bytes, &mut ctl)
    })?;
    let recipients = ids
        .into_iter()
        .zip(&rcv)
        .map(|(id, k)| (id, k.private.to_der().to_vec()))
        .collect();
    Ok(SentChunk {
        spec,
        digest,
        server,
        sender_id,
        sender_key: sender.private.to_der().to_vec(),
        recipients,
        expires,
    })
}
