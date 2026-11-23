//! Connection links, full and short (Agent/Protocol.hs, simplexmq 7.1.4, AGPL-3.0-only).
mod binary_req;
mod conn_req;
mod conn_req_data;
mod connection_link;
mod contact_type;
mod mode;
mod presets;
mod short_link;
mod short_link_bin;
mod short_link_parse;

pub use binary_req::BinaryConnReq;
pub use conn_req::{AddressRatchetKeys, ConnectionRequestUri};
pub use conn_req_data::{ConnReqUriData, ServiceScheme};
pub use connection_link::{ConnectionLink, CreatedConnLink};
pub use contact_type::ContactConnType;
pub use mode::ConnectionMode;
pub use presets::{restore_short_link, shorten_short_link};
pub use short_link::{ConnShortLink, ShortLinkScheme};
