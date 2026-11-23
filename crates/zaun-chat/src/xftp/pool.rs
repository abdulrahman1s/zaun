//! The connections one transfer holds, one per server, with retries on a fresh connection.
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::tls::TlsStream;

use super::address::XftpServer;
use super::client::{ClientConfig, XftpClient};
use super::connect::{TcpXftpClient, connect};
use super::error::XftpError;

pub type Client = XftpClient<TlsStream<TcpStream>>;

/// Holds at most one connection per server of the transfer and closes them all when dropped.
pub struct Pool {
    cfg: ClientConfig,
    clients: HashMap<XftpServer, TcpXftpClient>,
}

impl Pool {
    #[must_use]
    pub fn new(cfg: ClientConfig) -> Self {
        Self {
            cfg,
            clients: HashMap::new(),
        }
    }

    fn client(&mut self, server: &XftpServer) -> Result<&mut Client, XftpError> {
        if self
            .clients
            .get(server)
            .is_some_and(|c| !c.client.is_usable())
        {
            self.forget(server);
        }
        if !self.clients.contains_key(server) {
            self.clients
                .insert(server.clone(), connect(server, &self.cfg)?);
        }
        self.clients
            .get_mut(server)
            .map(|c| &mut c.client)
            .ok_or(XftpError::NoServers)
    }

    fn forget(&mut self, server: &XftpServer) {
        if let Some(mut c) = self.clients.remove(server) {
            c.close();
        }
    }

    /// Runs `f` on a connection to `server`; a temporary failure drops the connection and tries again, up to `attempts`.
    pub fn run<R>(
        &mut self,
        server: &XftpServer,
        cancel: &AtomicBool,
        attempts: usize,
        mut f: impl FnMut(&mut Client) -> Result<R, XftpError>,
    ) -> Result<R, XftpError> {
        let mut last = XftpError::NoServers;
        for _ in 0..attempts {
            if cancel.load(Ordering::Relaxed) {
                return Err(XftpError::Cancelled);
            }
            match self.client(server).and_then(&mut f) {
                Ok(r) => return Ok(r),
                Err(e) if e.is_temporary() => {
                    self.forget(server);
                    last = e;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for (_, mut c) in self.clients.drain() {
            c.close();
        }
    }
}
