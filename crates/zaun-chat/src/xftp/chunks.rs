//! Chunk sizes and how a file is split into them (FileTransfer/Chunks.hs, Client.hs `prepareChunkSizes`, `singleChunkSize`).

pub const KB: u64 = 1024;
pub const MB: u64 = 1024 * KB;
pub const GB: u64 = 1024 * MB;

pub const CHUNK_SIZE_0: u32 = 64 * 1024;
pub const CHUNK_SIZE_1: u32 = 256 * 1024;
pub const CHUNK_SIZE_2: u32 = 1024 * 1024;
pub const CHUNK_SIZE_3: u32 = 4 * 1024 * 1024;
pub const SERVER_CHUNK_SIZES: [u32; 4] = [CHUNK_SIZE_0, CHUNK_SIZE_1, CHUNK_SIZE_2, CHUNK_SIZE_3];

/// The soft limit XFTP clients report to the user (Description.hs `maxFileSize`).
pub const MAX_FILE_SIZE: u64 = GB;
/// What the agent refuses to prepare (`maxFileSizeHard`).
pub const MAX_FILE_SIZE_HARD: u64 = 5 * GB;
/// The Int64 length in front of an encrypted file (`fileSizeLen`).
pub const FILE_SIZE_LEN: u64 = 8;

fn three_quarters(size: u32) -> u64 {
    u64::from(size) * 3 / 4
}

/// The chunk sizes for an encrypted payload of `size` bytes: as many big chunks as fit, a remainder
/// above three quarters of a big chunk as one more, otherwise small chunks rounded up.
#[must_use]
pub fn prepare_chunk_sizes(size: u64) -> Vec<u32> {
    let (small, big) = if size > three_quarters(CHUNK_SIZE_3) {
        (CHUNK_SIZE_2, CHUNK_SIZE_3)
    } else if size > three_quarters(CHUNK_SIZE_2) {
        (CHUNK_SIZE_1, CHUNK_SIZE_2)
    } else {
        (CHUNK_SIZE_0, CHUNK_SIZE_1)
    };
    let mut sizes = Vec::new();
    let mut left = size;
    while left > 0 {
        if left >= u64::from(big) {
            let n = usize::try_from(left / u64::from(big)).unwrap_or(0);
            sizes.extend(std::iter::repeat_n(big, n));
            left %= u64::from(big);
        } else if left > three_quarters(big) {
            sizes.push(big);
            break;
        } else {
            let n = usize::try_from(left.div_ceil(u64::from(small))).unwrap_or(0);
            sizes.extend(std::iter::repeat_n(small, n));
            break;
        }
    }
    sizes
}

/// The smallest server chunk that holds `size` bytes, for a file that must be one chunk.
#[must_use]
pub fn single_chunk_size(size: u64) -> Option<u32> {
    SERVER_CHUNK_SIZES
        .into_iter()
        .find(|c| u64::from(*c) >= size)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkSpec {
    pub offset: u64,
    pub size: u32,
}

#[must_use]
pub fn prepare_chunk_specs(sizes: &[u32]) -> Vec<ChunkSpec> {
    sizes
        .iter()
        .scan(0u64, |offset, &size| {
            let spec = ChunkSpec {
                offset: *offset,
                size,
            };
            *offset += u64::from(size);
            Some(spec)
        })
        .collect()
}

/// The encrypted size of a file with `payload` bytes (header and content).
#[must_use]
pub fn encrypted_size(payload: u64) -> u64 {
    prepare_chunk_sizes(payload + FILE_SIZE_LEN + super::version::AUTH_TAG_SIZE as u64)
        .iter()
        .map(|s| u64::from(*s))
        .sum()
}
