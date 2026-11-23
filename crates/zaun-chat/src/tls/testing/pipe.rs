//! An in-memory duplex `Socket`; reads can be limited to a few bytes to exercise reassembly.
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use crate::net::Socket;

#[derive(Default)]
struct Lane {
    state: Mutex<(VecDeque<u8>, bool)>,
    ready: Condvar,
}

impl Lane {
    fn close(&self) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).1 = true;
        self.ready.notify_all();
    }
}

pub(in crate::tls) struct Pipe {
    rx: Arc<Lane>,
    tx: Arc<Lane>,
    chunk: usize,
}

impl Pipe {
    /// Two connected ends; `chunk` caps the bytes one read returns (0 for no cap).
    pub(in crate::tls) fn pair(chunk: usize) -> (Pipe, Pipe) {
        let (a, b) = (Arc::new(Lane::default()), Arc::new(Lane::default()));
        let chunk = if chunk == 0 { usize::MAX } else { chunk };
        (
            Pipe {
                rx: Arc::clone(&a),
                tx: Arc::clone(&b),
                chunk,
            },
            Pipe {
                rx: b,
                tx: a,
                chunk,
            },
        )
    }
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut g = self.rx.state.lock().unwrap_or_else(PoisonError::into_inner);
        while g.0.is_empty() && !g.1 {
            g = self
                .rx
                .ready
                .wait(g)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let n = buf.len().min(self.chunk).min(g.0.len());
        for slot in &mut buf[..n] {
            *slot = g.0.pop_front().unwrap_or_default();
        }
        Ok(n)
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut g = self.tx.state.lock().unwrap_or_else(PoisonError::into_inner);
        if g.1 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        g.0.extend(buf);
        self.tx.ready.notify_all();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Socket for Pipe {
    fn try_clone(&self) -> io::Result<Self> {
        Ok(Pipe {
            rx: Arc::clone(&self.rx),
            tx: Arc::clone(&self.tx),
            chunk: self.chunk,
        })
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        if matches!(how, Shutdown::Write | Shutdown::Both) {
            self.tx.close();
        }
        if matches!(how, Shutdown::Read | Shutdown::Both) {
            self.rx.close();
        }
        Ok(())
    }

    fn set_read_timeout(&self, _: Option<Duration>) -> io::Result<()> {
        Ok(())
    }
}
