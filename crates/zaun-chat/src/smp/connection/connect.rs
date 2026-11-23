//! One connect attempt over the server's hosts, in order (nook smp/client.ts `SmpClient.connect`).
use std::sync::Arc;

use crate::smp::error::{HandshakeError, SmpError};
use crate::smp::transport::{Closer, Connector, Wire};

use super::shared::{Shared, lock};

/// An identity or signature failure is the server's, not the network's: other hosts of the same address are not tried.
pub fn establish(sh: &Shared, connector: &dyn Connector) -> Result<Wire, SmpError> {
    let slot = |c: Arc<dyn Closer>| {
        *lock(&sh.closer) = Some(Arc::clone(&c));
        if sh.is_closed() {
            c.close();
        }
    };
    let mut last = SmpError::Address("no hosts");
    for host in &sh.server.hosts {
        if sh.is_closed() {
            return Err(SmpError::Closed);
        }
        match connector.connect(&sh.server, host, &sh.cfg, &slot) {
            Ok(wire) => return Ok(wire),
            Err(e @ SmpError::Handshake(HandshakeError::Identity | HandshakeError::BadAuth)) => {
                return Err(e);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}
