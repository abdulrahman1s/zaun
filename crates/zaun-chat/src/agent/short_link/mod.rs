//! Short link data: signed, encrypted blobs a server stores for a link (Crypto/ShortLink.hs, Agent/Protocol.hs, simplexmq 7.1.4, AGPL-3.0-only).
mod chat_data;
mod conn_data;
mod fixed;
mod kdf;
mod link_data;
mod owner;
mod seal;
mod user_data;

pub use chat_data::{ContactShortLinkData, decode_short_link_data, encode_short_link_data};
pub use conn_data::{ConnLinkData, UserConnLinkData, UserContactData};
pub use fixed::FixedLinkData;
pub use kdf::{contact_short_link_kdf, inv_short_link_kdf};
pub use link_data::{
    EncLinkData, decrypt_link_data, encode_sign_fixed_data, encode_sign_user_data,
    encrypt_fixed_data, encrypt_user_data,
};
pub use owner::{OwnerAuth, validate_link_owners};
pub use user_data::UserLinkData;
