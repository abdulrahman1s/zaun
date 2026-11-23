//! Chunks to the flat replica list of the YAML and back (Description.hs `unfoldChunksToReplicas`, `foldReplicasToChunks`).
use std::collections::BTreeMap;

use crate::encoding::{b64_decode, b64_encode};
use crate::xftp::address::XftpServer;

use super::file_size::{decode_file_size, encode_file_size};
use super::model::{ChunkReplica, FileChunk};

/// One line of a server's `chunks` list. The digest and a non-default size come with a chunk's first replica only.
pub(super) struct FlatReplica {
    pub(super) chunk_no: u32,
    pub(super) server: XftpServer,
    pub(super) replica_id: Vec<u8>,
    pub(super) replica_key: Vec<u8>,
    pub(super) digest: Option<Vec<u8>>,
    pub(super) chunk_size: Option<u32>,
}

pub(super) fn unfold(default_size: u32, chunks: &[FileChunk]) -> Vec<FlatReplica> {
    chunks
        .iter()
        .flat_map(|c| {
            c.replicas
                .iter()
                .enumerate()
                .map(move |(i, r)| FlatReplica {
                    chunk_no: c.chunk_no,
                    server: r.server.clone(),
                    replica_id: r.replica_id.clone(),
                    replica_key: r.replica_key.clone(),
                    digest: (i == 0).then(|| c.digest.clone()),
                    chunk_size: (i == 0 && c.chunk_size != default_size).then_some(c.chunk_size),
                })
        })
        .collect()
}

/// `groupAllWith server`: sorted by server, equal servers together, the original order kept inside a group.
pub(super) fn group_by_server(mut flat: Vec<FlatReplica>) -> Vec<(XftpServer, Vec<FlatReplica>)> {
    flat.sort_by(|a, b| a.server.cmp_haskell(&b.server));
    let mut groups: Vec<(XftpServer, Vec<FlatReplica>)> = Vec::new();
    for r in flat {
        match groups.last_mut() {
            Some((server, list)) if *server == r.server => list.push(r),
            _ => groups.push((r.server.clone(), vec![r])),
        }
    }
    groups
}

pub(super) fn encode_replica(r: &FlatReplica) -> String {
    let mut s = format!(
        "{}:{}:{}",
        r.chunk_no,
        b64_encode(&r.replica_id, true),
        b64_encode(&r.replica_key, true)
    );
    if let Some(d) = &r.digest {
        s.push(':');
        s.push_str(&b64_encode(d, true));
    }
    if let Some(size) = r.chunk_size {
        s.push(':');
        s.push_str(&encode_file_size(u64::from(size)));
    }
    s
}

pub(super) fn decode_replica(server: &XftpServer, s: &str) -> Result<FlatReplica, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if !(3..=5).contains(&parts.len()) {
        return Err(format!("bad chunk replica: {s}"));
    }
    let b64 = |p: &str| b64_decode(p).map_err(|_| format!("bad base64 in chunk replica: {s}"));
    Ok(FlatReplica {
        chunk_no: parts[0]
            .parse()
            .map_err(|_| format!("bad chunk number: {s}"))?,
        server: server.clone(),
        replica_id: b64(parts[1])?,
        replica_key: b64(parts[2])?,
        digest: parts.get(3).map(|p| b64(p)).transpose()?,
        chunk_size: parts
            .get(4)
            .map(|p| {
                decode_file_size(p)
                    .and_then(|n| u32::try_from(n).map_err(|_| format!("bad chunk size: {s}")))
            })
            .transpose()?,
    })
}

/// Fails when replicas of a chunk disagree on its size or digest, or when no replica names its digest.
pub(super) fn fold(default_size: u32, flat: Vec<FlatReplica>) -> Result<Vec<FileChunk>, String> {
    let mut sizes: BTreeMap<u32, u32> = BTreeMap::new();
    let mut digests: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for r in &flat {
        if let Some(size) = r.chunk_size
            && *sizes.entry(r.chunk_no).or_insert(size) != size
        {
            return Err("different size or digest in chunk replicas".into());
        }
        if let Some(d) = &r.digest
            && digests.entry(r.chunk_no).or_insert_with(|| d.clone()) != d
        {
            return Err("different size or digest in chunk replicas".into());
        }
    }
    let mut chunks: BTreeMap<u32, FileChunk> = BTreeMap::new();
    for r in flat {
        let replica = ChunkReplica {
            server: r.server,
            replica_id: r.replica_id,
            replica_key: r.replica_key,
        };
        match chunks.get_mut(&r.chunk_no) {
            Some(chunk) => chunk.replicas.push(replica),
            None => {
                let digest = digests
                    .get(&r.chunk_no)
                    .cloned()
                    .ok_or("no digest for chunk")?;
                let chunk_size = sizes.get(&r.chunk_no).copied().unwrap_or(default_size);
                chunks.insert(
                    r.chunk_no,
                    FileChunk {
                        chunk_no: r.chunk_no,
                        chunk_size,
                        digest,
                        replicas: vec![replica],
                    },
                );
            }
        }
    }
    Ok(chunks.into_values().collect())
}
