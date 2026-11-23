//! The stdout writer thread: each event as one line, flushed; the bounded channel is the backpressure.

use std::io::Write;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, bounded};
use zaun_chat_wire::{EvLine, Event, encode_line};

use crate::engine::log::Log;

/// Events handed over and not written yet.
pub const EVENTS: usize = 256;

pub fn spawn(
    stdout: impl Write + Send + 'static,
    log: Log,
) -> std::io::Result<(Sender<Event>, JoinHandle<()>)> {
    let (tx, rx) = bounded(EVENTS);
    let handle = std::thread::Builder::new()
        .name("chat-stdout".into())
        .spawn(move || write(stdout, &rx, &log))?;
    Ok((tx, handle))
}

/// Ends when the engine drops its sender, or when core stops reading (dropping `rx` tells the engine).
fn write(mut stdout: impl Write, rx: &Receiver<Event>, log: &Log) {
    for ev in rx {
        let line = match encode_line(&EvLine { ev }) {
            Ok(l) => l,
            Err(e) => {
                log.line(format_args!("an event could not be written: {e}"));
                continue;
            }
        };
        if writeln!(stdout, "{line}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            return;
        }
    }
}
