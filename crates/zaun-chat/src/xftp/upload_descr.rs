//! Descriptions of an uploaded file: the sender's, to delete it, and one per recipient (Agent.hs `sndFileToDescrs`).
use super::description::{ChunkReplica, FileChunk, FileDescription, Party};
use super::upload_chunk::SentChunk;
use super::upload_prepare::Prepared;

fn chunks(
    sent: &[SentChunk],
    replica: impl Fn(&SentChunk) -> Option<ChunkReplica>,
) -> Option<Vec<FileChunk>> {
    sent.iter()
        .zip(1u32..)
        .map(|(c, chunk_no)| {
            Some(FileChunk {
                chunk_no,
                chunk_size: c.spec.size,
                digest: c.digest.clone(),
                replicas: vec![replica(c)?],
            })
        })
        .collect()
}

fn description(
    party: Party,
    prepared: &Prepared,
    sent: &[SentChunk],
    chunks: Vec<FileChunk>,
) -> FileDescription {
    FileDescription {
        party,
        size: prepared.enc_size,
        digest: prepared.digest.clone(),
        key: prepared.key.as_bytes().to_vec(),
        nonce: prepared.nonce.as_bytes().to_vec(),
        chunk_size: sent.first().map_or(0, |c| c.spec.size),
        chunks,
        redirect: None,
    }
}

/// The sender description and one recipient description per recipient key of each chunk.
pub(super) fn describe(
    prepared: &Prepared,
    sent: &[SentChunk],
    recipients: usize,
) -> Option<(FileDescription, Vec<FileDescription>)> {
    let sender_chunks = chunks(sent, |c| {
        Some(ChunkReplica {
            server: c.server.without_auth(),
            replica_id: c.sender_id.clone(),
            replica_key: c.sender_key.clone(),
        })
    })?;
    let sender = description(Party::Sender, prepared, sent, sender_chunks);
    let rcvs = (0..recipients)
        .map(|n| {
            let cs = chunks(sent, |c| {
                let (id, key) = c.recipients.get(n)?;
                Some(ChunkReplica {
                    server: c.server.without_auth(),
                    replica_id: id.clone(),
                    replica_key: key.clone(),
                })
            })?;
            Some(description(Party::Recipient, prepared, sent, cs))
        })
        .collect::<Option<Vec<_>>>()?;
    Some((sender, rcvs))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::crypto::{CbNonce, SbKey};
    use crate::xftp::chunks::ChunkSpec;
    use crate::xftp::description::encode_description;

    use super::*;

    #[test]
    fn a_servers_password_never_reaches_a_description() {
        let server = format!(
            "xftp://{}@files.example:5443",
            "da1aH3nOT-9G8lV7bWamhxpDYdJ1xmW7j3JpGaDR5Ug=:s3cret"
        )
        .parse()
        .unwrap();
        let prepared = Prepared {
            path: PathBuf::from("/nonexistent/zaun-test"),
            key: SbKey::new([1; 32]),
            nonce: CbNonce::new([2; 24]),
            digest: vec![3; 64],
            enc_size: 65536,
            specs: vec![ChunkSpec {
                offset: 0,
                size: 65536,
            }],
        };
        let sent = [SentChunk {
            spec: ChunkSpec {
                offset: 0,
                size: 65536,
            },
            digest: vec![4; 32],
            server,
            sender_id: b"snd".to_vec(),
            sender_key: vec![5; 48],
            recipients: vec![(b"rcv".to_vec(), vec![6; 48])],
            expires: None,
        }];
        let (sender, recipients) = describe(&prepared, &sent, 1).unwrap();
        for fd in recipients.iter().chain([&sender]) {
            let text = encode_description(fd);
            assert!(!text.contains("s3cret"), "{text}");
            assert!(text.contains("@files.example:5443"), "{text}");
        }
    }
}
