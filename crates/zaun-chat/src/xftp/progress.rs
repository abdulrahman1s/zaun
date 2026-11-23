//! How a long transfer reports progress and learns it should stop.
use std::sync::atomic::{AtomicBool, Ordering};

use super::error::XftpError;

/// Bytes of the encrypted file moved so far and in total; called often, so the caller throttles.
pub type ProgressFn<'a> = dyn FnMut(u64, u64) + 'a;

/// A flag the engine sets to cancel; checked between steps and on every network wait.
#[derive(Debug, Clone, Copy)]
pub struct CancelFlag<'a>(pub &'a AtomicBool);

impl CancelFlag<'_> {
    pub fn check(&self) -> Result<(), XftpError> {
        if self.0.load(Ordering::Relaxed) {
            Err(XftpError::Cancelled)
        } else {
            Ok(())
        }
    }
}
