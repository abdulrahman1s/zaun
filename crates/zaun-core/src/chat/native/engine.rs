//! A running helper: requests go to the writer; dropping it stops the helper.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::Duration;

use crate::chat::engine::{Engine, Request};

/// How long a stopping helper may take to close its database before it is killed.
pub(super) const STOP_GRACE: Duration = Duration::from_secs(5);

pub(super) struct NativeEngine {
    pub(super) tx: Option<SyncSender<Request>>,
    pub(super) pid: u32,
    pub(super) exited: Arc<AtomicBool>,
}

impl Engine for NativeEngine {
    fn request(&self, request: Request) {
        let Some(tx) = &self.tx else { return };
        match tx.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!("chat: the helper is not reading; a request was dropped")
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::debug!("chat: the helper is gone; a request was dropped")
            }
        }
    }

    fn shutdown(&mut self) {
        self.tx = None;
        let step = Duration::from_millis(50);
        let mut waited = Duration::ZERO;
        while !self.exited.load(Ordering::SeqCst) && waited < STOP_GRACE {
            std::thread::sleep(step);
            waited += step;
        }
    }
}

impl Drop for NativeEngine {
    fn drop(&mut self) {
        // Closing the channel ends the writer, which closes the helper's input.
        self.tx = None;
        let exited = Arc::clone(&self.exited);
        let pid = self.pid;
        let _ = std::thread::Builder::new()
            .name("chat-stop".into())
            .spawn(move || {
                std::thread::sleep(STOP_GRACE);
                if !exited.load(Ordering::SeqCst)
                    && let Some(pid) = i32::try_from(pid)
                        .ok()
                        .and_then(rustix::process::Pid::from_raw)
                {
                    // The reader thread still holds the child unreaped, so the PID is ours.
                    let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
                }
            });
    }
}
