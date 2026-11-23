//! A minimal synchronous HTTP/2 client for XFTP (RFC 9113, HPACK RFC 7541); ported from nook src/sx/h2 (AGPL-3.0-only).

mod connection;
mod error;
pub mod frames;
mod hpack_decode;
mod hpack_encode;
mod hpack_int;
mod huffman;
mod huffman_table;
mod reader;
mod recv;
mod recv_data;
mod recv_headers;
mod response;
mod send;
mod state;
mod static_table;

pub use connection::Connection;
pub use error::{H2Error, Stop};
pub use hpack_decode::{Headers, HpackDecoder};
pub use hpack_encode::encode_headers;
pub use hpack_int::{HpackError, decode_int, encode_int};
pub use huffman::{Huffman, huffman_encode};
pub use response::{Progress, Request, Response};
pub use state::Options;
