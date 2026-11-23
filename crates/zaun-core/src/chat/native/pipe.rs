//! The helper's pipes: a writer thread (requests in, as lines) and a reader thread (lines
//! out, as events) that also owns the child and reports its end (ADR-0012: no async).

use std::io::{BufRead, BufReader, Write};
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use zaun_chat_wire::{EvLine, ReqLine, decode_line, encode_line};

use super::from_wire::event;
use super::status::exit_events;
use super::to_wire::request;
use crate::chat::engine::{ChatEvent, EventSink, Request};
use crate::error::Result;

/// Requests waiting for the writer; a helper that stops reading fills it, and the service
/// thread must not block on that.
pub(super) const QUEUE: usize = 1024;

/// What [`start`] sets running.
pub(super) struct Pipes {
    pub tx: SyncSender<Request>,
    /// Set once the reader has seen the helper end.
    pub exited: Arc<AtomicBool>,
}

/// Start the threads on `child`'s pipes; `hello` is the first line the helper reads.
pub(super) fn start(mut child: Child, hello: String, sink: EventSink) -> Result<Pipes> {
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        bail!(ChtUnavailable);
    };
    let (tx, rx) = sync_channel::<Request>(QUEUE);
    spawn("chat-write", move || write(stdin, &hello, rx))?;
    let exited = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&exited);
    spawn("chat-read", move || read(stdout, child, &sink, &done))?;
    Ok(Pipes { tx, exited })
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map(drop)
        .map_err(|e| err!(IntTaskFailed).with_source(e))
}

fn write(mut stdin: impl Write, hello: &str, rx: Receiver<Request>) {
    let mut put = |line: &str| writeln!(stdin, "{line}").and_then(|()| stdin.flush());
    if put(hello).is_err() {
        return;
    }
    for r in rx {
        let line = match encode_line(&ReqLine { req: request(r) }) {
            Ok(line) => line,
            Err(e) => {
                tracing::warn!(error = %e, "chat: a request could not be written");
                continue;
            }
        };
        if put(&line).is_err() {
            break;
        }
    }
    // Dropping stdin closes it: the helper closes its database and exits.
}

fn read(stdout: impl std::io::Read, mut child: Child, sink: &EventSink, exited: &AtomicBool) {
    let mut started = false;
    for line in BufReader::new(stdout).split(b'\n') {
        let Ok(line) = line else { break };
        let line = String::from_utf8_lossy(&line);
        let ev = match decode_line::<EvLine>(&line) {
            Ok(l) => event(l.ev),
            Err(e) => {
                // Never the line: it may hold a message.
                tracing::warn!(error = %e, len = line.len(), "chat: a line from the helper was skipped");
                continue;
            }
        };
        match ev {
            // The reader reports the real end below.
            ChatEvent::Exited { .. } => {}
            ChatEvent::Ready(_) => {
                started = true;
                sink(ev);
            }
            _ => sink(ev),
        }
    }
    let status = child.wait().ok().and_then(|s| s.code());
    exited.store(true, Ordering::SeqCst);
    for ev in exit_events(status, started) {
        sink(ev);
    }
}
