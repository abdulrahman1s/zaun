//! The speaker thread: decoded 20 ms frames from the session, written to the device.
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use zaun_core::error::ErrorCode;

use super::backend::AudioBackend;
use crate::session::Inbox;

pub(crate) fn spawn(
    backend: Arc<dyn AudioBackend>,
    device: String,
    frames: Receiver<Vec<i16>>,
    tx: Sender<Inbox>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("call-speaker".into())
        .spawn(move || {
            let mut out = match backend.open_output(&device) {
                Ok(o) => o,
                Err(e) => {
                    let detail = format!("the speakers could not be opened: {e}");
                    let _ = tx.send(Inbox::Failed {
                        code: ErrorCode::ChtMediaFailed,
                        detail,
                    });
                    return;
                }
            };
            // Ends when the session drops its sender.
            for frame in frames {
                if let Err(e) = out.write(&frame) {
                    let _ = tx.send(Inbox::Failed {
                        code: ErrorCode::ChtMediaFailed,
                        detail: format!("the speakers stopped: {e}"),
                    });
                    return;
                }
            }
        })?;
    Ok(())
}
