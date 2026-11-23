//! Blocks between a client and the in-memory server over channels, closed from either end.
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::smp::{Closer, Reader, Writer};

pub struct PipeReader {
    pub rx: Receiver<Vec<u8>>,
    pub closed: Arc<AtomicBool>,
}

pub struct PipeWriter {
    pub tx: Sender<Vec<u8>>,
    pub closed: Arc<AtomicBool>,
}

pub struct PipeCloser(pub Arc<AtomicBool>);

impl Reader for PipeReader {
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        loop {
            if self.closed.load(Ordering::Acquire) {
                return Err(io::ErrorKind::ConnectionReset.into());
            }
            match self.rx.recv_timeout(Duration::from_millis(5)) {
                Ok(b) => {
                    buf.copy_from_slice(&b);
                    return Ok(());
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
            }
        }
    }

    fn set_read_timeout(&mut self, _: Option<Duration>) -> io::Result<()> {
        Ok(())
    }
}

impl Writer for PipeWriter {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.tx
            .send(buf.to_vec())
            .map_err(|_| io::ErrorKind::BrokenPipe.into())
    }
}

impl Closer for PipeCloser {
    fn close(&self) {
        self.0.store(true, Ordering::Release);
    }
}
