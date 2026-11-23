//! The YAML file description exchanged in chat, matching Haskell byte for byte (FileTransfer/Description.hs; nook xftp/description.ts, AGPL-3.0-only).

mod decode;
mod file_size;
mod model;
mod parts;
mod replicas;
mod validate;
mod yaml_node;
mod yaml_parse;
mod yaml_print;

pub use file_size::{decode_file_size, encode_file_size};
pub use model::{ChunkReplica, FileChunk, FileDescription, Party, Redirect};
pub use parts::{
    BADGE_DESCR_PART_SIZE, DESCR_PART_SIZE, DescrAssembler, DescrPart, join_description,
    split_description,
};
pub use yaml_print::encode_description;
