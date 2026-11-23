//! Plain TCP under the SMP and XFTP transports (ADR-0042); the socket seam keeps TLS testable.

mod connect;
mod error;
mod socket;

pub use connect::connect_tcp;
pub use error::NetError;
pub use socket::Socket;
