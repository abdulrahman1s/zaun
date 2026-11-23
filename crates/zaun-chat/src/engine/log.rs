//! The helper's log on standard error: one line per entry, never keys or message text.

use std::fmt;
use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone)]
pub struct Log(Arc<Mutex<Box<dyn Write + Send>>>);

impl Log {
    pub fn new(sink: impl Write + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Box::new(sink))))
    }

    /// A log that keeps nothing.
    #[must_use]
    pub fn discard() -> Self {
        Self::new(std::io::sink())
    }

    pub fn line(&self, args: fmt::Arguments<'_>) {
        let mut w = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        // A log that cannot be written is no reason to stop chatting.
        let _ = writeln!(w, "zaun-chat: {args}").and_then(|()| w.flush());
    }
}

impl fmt::Debug for Log {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Log")
    }
}
